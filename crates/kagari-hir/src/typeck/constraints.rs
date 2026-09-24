use std::collections::HashMap;

use kagari_common::{Diagnostic, DiagnosticKind, cancellation::CancellationToken};
use smallvec::SmallVec;

use crate::{
    builtin::surface::{self, StandardTypeConstraint},
    hir,
    lower::LoweredModule,
    types::TypeId,
};

use super::{
    ConstraintTarget, ResolvedTypeRef, TypeTable, TypeTarget,
    ty::{TypeContext, resolve_type_in},
};

/// Resolve bounds once in their declaring context, before signatures and bodies.
pub(super) fn resolve_constraints(
    lowered: &LoweredModule,
    declarations: &crate::declarations::Declarations,
    table: &mut TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
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
                },
                table,
                diagnostics,
                cancel,
                true,
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
}

fn resolve_owner(
    lowered: &LoweredModule,
    generics: &[hir::GenericParam],
    bounds: &[hir::TraitBound],
    declarations: &crate::declarations::Declarations,
    table: &mut TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    let context = TypeContext {
        declarations,
        generics,
        self_type: None,
    };
    for param in generics {
        for reference in &param.bounds {
            resolve_constraint(
                lowered,
                reference,
                context,
                table,
                diagnostics,
                cancel,
                false,
            );
        }
    }
    for bound in bounds {
        if cancel.check().is_err() {
            return;
        }
        resolve_type_in(&lowered.module, bound.target_ref, context, table, cancel);
        if !matches!(
            table
                .type_ref(bound.target_ref)
                .and_then(|reference| reference.target),
            Some(TypeTarget::Generic(_))
        ) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidBoundTarget {
                    name: bound.target.clone(),
                })
                .with_span(lowered.source_map.type_span(bound.target_ref)),
            );
        }
        for reference in &bound.traits {
            resolve_constraint(
                lowered,
                reference,
                context,
                table,
                diagnostics,
                cancel,
                false,
            );
        }
    }
}

fn resolve_constraint(
    lowered: &LoweredModule,
    reference: &hir::TraitRef,
    context: TypeContext<'_>,
    table: &mut TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
    allow_applied: bool,
) {
    if cancel.check().is_err() || table.has_constraint(reference.ty) {
        return;
    }
    // Impl headers retain applied trait arguments; bounds still reject applications
    // until their target identity includes the applied arguments.
    let (name, applied) = match &lowered.module.type_ref(reference.ty).kind {
        hir::TypeKind::Generic { name, args } => {
            if !allow_applied {
                for arg in args {
                    resolve_type_in(&lowered.module, *arg, context, table, cancel);
                }
            }
            (name, true)
        }
        hir::TypeKind::Named(name) => (name, false),
        _ => unreachable!("trait references have a named base"),
    };
    let mut resolved = if applied && allow_applied {
        resolve_type_in(&lowered.module, reference.ty, context, table, cancel);
        table
            .type_ref(reference.ty)
            .cloned()
            .expect("resolved trait application")
    } else {
        super::ty::resolve_named_type(name, context)
    };
    if applied
        && allow_applied
        && let hir::TypeKind::Generic { args, .. } = &lowered.module.type_ref(reference.ty).kind
    {
        for argument in args {
            if table
                .type_ref(*argument)
                .is_some_and(|resolved| resolved.ty.is_unresolved())
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                        type_name: super::ty::display_type(&lowered.module, *argument),
                    })
                    .with_span(lowered.source_map.type_span(*argument)),
                );
            }
        }
    }
    // Standard constraints are a fallback, so a binder or explicit declaration
    // cannot resolve differently here and in an ordinary type annotation.
    let standard = (resolved.target.is_none() && context.declarations.names.lookup(name).is_none())
        .then(|| surface::standard_constraint(name))
        .flatten();
    let target = standard
        .map(ConstraintTarget::Standard)
        .or(match resolved.target {
            Some(TypeTarget::Trait(id)) if matches!(resolved.ty, TypeId::Trait(_)) => context
                .declarations
                .definition(crate::resolver::ResolvedName::Trait(id))
                .cloned()
                .map(ConstraintTarget::Trait),
            _ => None,
        });
    let reason = if applied && !allow_applied {
        Some("generic trait applications require concrete instantiation")
    } else if applied && !matches!(resolved.ty, TypeId::Trait(_)) {
        Some("invalid generic trait application")
    } else if !applied
        && matches!(resolved.target, Some(TypeTarget::Trait(id)) if lowered.module.traits.iter().any(|item| item.id == id && !item.generic_params.is_empty()))
    {
        Some("generic trait references require concrete type arguments")
    } else if matches!(resolved.ty, TypeId::Trait(_)) && target.is_none() {
        Some("imported trait constraints and implementations are not yet supported")
    } else if !resolved.ty.is_unresolved() && target.is_none() {
        Some("expected a trait, not another type or generic parameter")
    } else {
        None
    };
    if applied && !allow_applied {
        resolved.ty = TypeId::Error;
    }
    if standard.is_none() || applied {
        table.insert_type_ref(reference.ty, resolved);
    }
    let target = target.filter(|_| reason.is_none());
    table.insert_constraint(reference.ty, target.clone());
    if target.is_none() {
        if table.type_ref(reference.ty).is_none() {
            table.insert_type_ref(
                reference.ty,
                ResolvedTypeRef {
                    ty: TypeId::Error,
                    target: None,
                },
            );
        }
        diagnostics.push(
            Diagnostic::error(if let Some(reason) = reason {
                DiagnosticKind::InvalidTraitReference {
                    trait_name: super::ty::display_type(&lowered.module, reference.ty),
                    reason,
                }
            } else {
                DiagnosticKind::UnknownTrait {
                    trait_name: super::ty::display_type(&lowered.module, reference.ty),
                }
            })
            .with_span(lowered.source_map.type_span(reference.ty)),
        );
    }
}

/// Keep constraints attached to the declaring parameter, including when an
/// implicit receiver still contains an outer parameter shadowed by the method.
pub(super) fn function_bounds(
    module: &hir::Module,
    function: &hir::Function,
    declarations: &crate::declarations::Declarations,
    table: &TypeTable,
) -> HashMap<crate::types::GenericParameterType, Vec<ConstraintTarget>> {
    let mut result = parameter_bounds(&function.generic_params, declarations, table);
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
        let Some(TypeTarget::Generic(id)) = table
            .type_ref(bound.target_ref)
            .and_then(|reference| reference.target)
        else {
            continue;
        };
        let Some(param) = declarations.generic_type(id) else {
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

pub(super) fn implementation_bounds(
    implementation: &hir::Impl,
    declarations: &crate::declarations::Declarations,
    table: &TypeTable,
) -> super::GenericBounds {
    let mut result = parameter_bounds(&implementation.generic_params, declarations, table);
    for bound in &implementation.bounds {
        let Some(TypeTarget::Generic(id)) = table
            .type_ref(bound.target_ref)
            .and_then(|reference| reference.target)
        else {
            continue;
        };
        let Some(parameter) = declarations.generic_type(id) else {
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
    params: &[hir::GenericParam],
    declarations: &crate::declarations::Declarations,
    table: &TypeTable,
) -> super::GenericBounds {
    params
        .iter()
        .filter_map(|param| {
            Some((
                declarations.generic_type(param.id)?,
                param
                    .bounds
                    .iter()
                    .filter_map(|reference| table.constraint(reference.ty))
                    .collect(),
            ))
        })
        .collect()
}

pub(super) fn type_satisfies_standard_constraint(
    ty: &TypeId,
    constraint: StandardTypeConstraint,
    bounds: &super::GenericBounds,
) -> bool {
    match ty {
        TypeId::Tuple(members) | TypeId::StandardEnum { args: members, .. }
            if constraint == StandardTypeConstraint::Comparable =>
        {
            members
                .iter()
                .all(|ty| type_satisfies_standard_constraint(ty, constraint, bounds))
        }
        TypeId::Generic(name) => bounds
            .get(name)
            .is_some_and(|bounds| bounds.contains(&super::ConstraintTarget::Standard(constraint))),
        _ => match constraint {
            StandardTypeConstraint::HashKey => surface::supports_hash_key(ty),
            StandardTypeConstraint::Iterable => surface::iterable_protocol(ty).is_some(),
            StandardTypeConstraint::OrderedNumber => surface::supports_ordering(ty, ty),
            StandardTypeConstraint::SignedNumber => surface::supports_unary_negation(ty),
            StandardTypeConstraint::Comparable => ty.supports_equality(),
        },
    }
}

/// Recovery holes do not decide a constraint, but known siblings still can fail it.
pub(super) fn known_type_violates_constraint(
    ty: &TypeId,
    constraint: StandardTypeConstraint,
    bounds: &super::GenericBounds,
) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            TypeId::Unknown | TypeId::Error => {}
            TypeId::Tuple(members) | TypeId::StandardEnum { args: members, .. }
                if constraint == StandardTypeConstraint::Comparable =>
            {
                pending.extend(members)
            }
            _ if !type_satisfies_standard_constraint(ty, constraint, bounds) => return true,
            _ => {}
        }
    }
    false
}

pub(super) fn standard_constraint_reason(constraint: StandardTypeConstraint) -> &'static str {
    match constraint {
        StandardTypeConstraint::HashKey => {
            "only bool, integer, and String keys have specified hash semantics"
        }
        StandardTypeConstraint::Iterable => "type is not part of the standard iterable protocol",
        StandardTypeConstraint::OrderedNumber => "type is not an ordered numeric type",
        StandardTypeConstraint::SignedNumber => "type is not a signed numeric type",
        StandardTypeConstraint::Comparable => "type does not have standard equality semantics",
    }
}
