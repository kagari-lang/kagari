//! Resolve written generic bounds into ordinary applied trait requirements.

use crate::{
    declarations::Declarations,
    hir::{
        ids::GenericParamId,
        item::{
            Module,
            behavior::{Impl, TraitBound, TraitRef},
            function::Function,
        },
        ty::TypeKind,
    },
    lower::LoweredModule,
    typeck::{
        check::function_type_context,
        state::TypeCheckState,
        table::{ResolvedTypeTarget, TypeTable},
        ty,
        ty::{TypeContext, resolve_type_in},
    },
    types::TypeId,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;

/// Resolve bounds once in their declaring context, before signatures and bodies.
pub(super) fn resolve_constraints(
    lowered: &LoweredModule,
    declarations: &Declarations,
    table: &mut TypeCheckState,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    for item in &lowered.module.native_types {
        resolve_owner(
            lowered,
            &item.generic_params,
            &item.bounds,
            declarations,
            table,
            diagnostics,
            cancel,
        );
    }
    for params in lowered
        .module
        .structs
        .iter()
        .map(|item| &item.generic_params)
        .chain(lowered.module.enums.iter().map(|item| &item.generic_params))
    {
        resolve_owner(
            lowered,
            params,
            &[],
            declarations,
            table,
            diagnostics,
            cancel,
        );
    }
    for item in &lowered.module.traits {
        for reference in &item.supertraits {
            resolve_constraint(
                lowered,
                reference,
                TypeContext {
                    declarations,
                    generics: &item.generic_params,
                    self_type: Some(item.id),
                    implementation: None,
                },
                table,
                diagnostics,
                cancel,
            );
        }
        resolve_owner(
            lowered,
            &item.generic_params,
            &[],
            declarations,
            table,
            diagnostics,
            cancel,
        );
    }
    for item in &lowered.module.impls {
        if let Some(reference) = &item.trait_ref {
            resolve_constraint(
                lowered,
                reference,
                TypeContext {
                    declarations,
                    generics: &item.generic_params,
                    self_type: None,
                    implementation: None,
                },
                table,
                diagnostics,
                cancel,
            );
        }
        resolve_owner(
            lowered,
            &item.generic_params,
            &item.bounds,
            declarations,
            table,
            diagnostics,
            cancel,
        );
    }
    for item in &lowered.module.functions {
        resolve_owner_in(
            lowered,
            &item.bounds,
            function_type_context(&lowered.module, item, declarations),
            table,
            diagnostics,
            cancel,
        );
    }
}

pub(super) fn resolve_owner(
    lowered: &LoweredModule,
    generics: &[GenericParamId],
    bounds: &[TraitBound],
    declarations: &Declarations,
    table: &mut TypeCheckState,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    let context = TypeContext {
        declarations,
        generics,
        self_type: None,
        implementation: None,
    };
    resolve_owner_in(lowered, bounds, context, table, diagnostics, cancel);
}

pub(super) fn resolve_owner_in(
    lowered: &LoweredModule,
    bounds: &[TraitBound],
    context: TypeContext<'_>,
    table: &mut TypeCheckState,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    let generics = context.generics;
    for &id in generics {
        for reference in &lowered.module.generic_parameter(id).bounds {
            resolve_constraint(lowered, reference, context, table, diagnostics, cancel);
        }
    }
    for bound in bounds {
        for reference in &bound.traits {
            resolve_constraint(lowered, reference, context, table, diagnostics, cancel);
        }
    }
    for bound in bounds {
        if cancel.check().is_err() {
            return;
        }
        resolve_type_in(&lowered.module, bound.target_ref, context, table, cancel);
        // Fixed and applied receivers are ordinary predicates too. Resolving a
        // predicate does not prove it: closed predicates are checked against the
        // completed catalog, and open ones at application sites.
        if table
            .type_ref(bound.target_ref)
            .is_none_or(|reference| reference.ty.is_unresolved())
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidBoundTarget {
                    name: bound.target.clone(),
                })
                .with_span(
                    lowered
                        .module
                        .type_ref(bound.target_ref)
                        .origin
                        .diagnostic_span(),
                ),
            );
        }
        for reference in &bound.traits {
            resolve_constraint(lowered, reference, context, table, diagnostics, cancel);
        }
    }
}

pub(super) fn resolve_constraint(
    lowered: &LoweredModule,
    reference: &TraitRef,
    context: TypeContext<'_>,
    table: &mut TypeCheckState,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    if cancel.check().is_err() || table.has_constraint(reference.ty) {
        return;
    }
    // Every trait constraint retains its applied type arguments in its identity.
    let (name, applied) = match &lowered.module.type_ref(reference.ty).kind {
        TypeKind::Generic { name, .. } | TypeKind::Definition { name, .. } => (name, true),
        TypeKind::Named(name) => (name, false),
        _ => unreachable!("trait references have a named base"),
    };
    let resolved = if applied {
        resolve_type_in(&lowered.module, reference.ty, context, table, cancel);
        table
            .type_ref(reference.ty)
            .cloned()
            .expect("resolved trait application")
    } else {
        ty::resolve_named_type(&lowered.module, name, context)
    };
    if applied
        && let TypeKind::Generic { args, .. } | TypeKind::Definition { args, .. } =
            &lowered.module.type_ref(reference.ty).kind
    {
        for argument in args {
            if table
                .type_ref(*argument)
                .is_some_and(|resolved| resolved.ty.is_unresolved())
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                        type_name: ty::display_type(&lowered.module, *argument),
                    })
                    .with_span(lowered.module.type_ref(*argument).origin.diagnostic_span()),
                );
            }
        }
    }
    let target = match &resolved.ty {
        TypeId::Trait(instance)
            if context
                .declarations
                .definition_target(&instance.declaration)
                .is_some()
                || context
                    .declarations
                    .imported_types()
                    .by_declaration(&instance.declaration)
                    .is_some() =>
        {
            Some(instance.clone())
        }
        _ => None,
    };
    let reason = if matches!(&resolved.target, Some(ResolvedTypeTarget::Source(id)) if context.declarations.imported_types().alias(id).is_some())
    {
        Some("a type alias names a value type, not a trait bound")
    } else if applied && !matches!(resolved.ty, TypeId::Trait(_)) {
        Some("invalid generic trait application")
    } else if !applied
        && matches!(&resolved.ty, TypeId::Trait(instance) if !instance.arguments.is_empty())
    {
        Some("generic trait references require concrete type arguments")
    } else if !resolved.ty.is_unresolved() && target.is_none() {
        Some("expected a trait, not another type or generic parameter")
    } else {
        None
    };
    let inaccessible = resolved.ty.is_unresolved()
        && resolved.target.is_none()
        && context
            .declarations
            .trait_path_inaccessible(&lowered.module.type_ref(reference.ty).kind, cancel);
    table.insert_type_ref(reference.ty, resolved);
    let target = target.filter(|_| reason.is_none());
    table.insert_constraint(reference.ty, target.clone());
    if target.is_none() {
        diagnostics.push(
            Diagnostic::error(if inaccessible {
                DiagnosticKind::ImportNotPublic { path: name.clone() }
            } else if let Some(reason) = reason {
                DiagnosticKind::InvalidTraitReference {
                    trait_name: ty::display_type(&lowered.module, reference.ty),
                    reason,
                }
            } else {
                DiagnosticKind::UnknownTrait {
                    trait_name: ty::display_type(&lowered.module, reference.ty),
                }
            })
            .with_span(
                lowered
                    .module
                    .type_ref(reference.ty)
                    .origin
                    .diagnostic_span(),
            ),
        );
    }
}

/// Keep constraints attached to the declaring parameter, including when an
/// implicit receiver still contains an outer parameter shadowed by the method.
pub(super) fn function_bounds(
    module: &Module,
    function: &Function,
    declarations: &Declarations,
    table: &TypeTable,
) -> super::GenericBounds {
    let mut result = parameter_bounds(module, &function.generic_params, declarations, table);
    let inherited = module
        .impls
        .iter()
        .find(|item| {
            item.methods
                .iter()
                .any(|method| method.function == function.id)
        })
        .map(|item| item.bounds.as_slice())
        .unwrap_or_default();
    for bound in inherited.iter().chain(&function.bounds) {
        let Some(param) = table
            .type_ref(bound.target_ref)
            .map(|reference| reference.ty.clone())
        else {
            continue;
        };
        result.entry(param).or_default().extend(
            bound
                .traits
                .iter()
                .filter_map(|reference| table.constraint(reference.ty)),
        );
    }
    result
}

pub(crate) fn implementation_bounds(
    module: &Module,
    implementation: &Impl,
    declarations: &Declarations,
    table: &TypeTable,
) -> super::GenericBounds {
    let mut result = parameter_bounds(module, &implementation.generic_params, declarations, table);
    for bound in &implementation.bounds {
        let Some(parameter) = table
            .type_ref(bound.target_ref)
            .map(|reference| reference.ty.clone())
        else {
            continue;
        };
        result.entry(parameter).or_default().extend(
            bound
                .traits
                .iter()
                .filter_map(|reference| table.constraint(reference.ty)),
        );
    }
    result
}

pub(super) fn parameter_bounds(
    module: &Module,
    params: &[GenericParamId],
    declarations: &Declarations,
    table: &TypeTable,
) -> super::GenericBounds {
    params
        .iter()
        .filter_map(|param| {
            Some((
                TypeId::Generic(declarations.generic_type(*param)?),
                module
                    .generic_parameter(*param)
                    .bounds
                    .iter()
                    .filter_map(|reference| table.constraint(reference.ty))
                    .collect(),
            ))
        })
        .collect()
}
