use crate::{
    builtin::{surface, traits::intrinsic_holds},
    declarations::Declarations,
    hir::{
        item::{
            Module,
            behavior::{GenericParam, Impl, TraitBound, TraitRef},
            function::Function,
        },
        ty::TypeKind,
    },
    lower::LoweredModule,
    typeck::{
        check::function_type_context,
        table::{ConstraintTarget, ResolvedTypeRef, TypeTable, TypeTarget},
        ty::{self, TypeContext, resolve_type_in},
    },
    types::TypeId,
};

use kagari_abi::standard::{surface::StandardTypeConstraint, traits::StandardTrait};
use kagari_common::{
    cancellation::CancellationToken,
    diagnostic::{Diagnostic, DiagnosticKind},
};
use smallvec::SmallVec;

/// Resolve bounds once in their declaring context, before signatures and bodies.
pub(super) fn resolve_constraints(
    lowered: &LoweredModule,
    declarations: &Declarations,
    table: &mut TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    for item in &lowered.module.opaque_types {
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
    generics: &[GenericParam],
    bounds: &[TraitBound],
    declarations: &Declarations,
    table: &mut TypeTable,
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
    table: &mut TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    let generics = context.generics;
    for param in generics {
        for reference in &param.bounds {
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
        if !matches!(
            table
                .type_ref(bound.target_ref)
                .and_then(|reference| reference.target.clone()),
            Some(TypeTarget::Generic(_))
        ) && !matches!(
            table.type_ref(bound.target_ref).map(|reference| (&reference.ty, reference.target.clone())),
            Some((TypeId::SelfType(_), Some(TypeTarget::Trait(id)))) if Some(id) == context.self_type
        ) && !(table
            .type_ref(bound.target_ref)
            .is_some_and(|reference| !reference.ty.is_unresolved())
            && match &lowered.module.type_ref(bound.target_ref).kind {
                TypeKind::Projection { .. } => true,
                TypeKind::Named(name) => name.split_once("::").is_some_and(|(base, _)| {
                    generics.iter().any(|param| param.name == base)
                        || base == "Self" && context.self_type.is_some()
                }),
                _ => false,
            })
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidBoundTarget {
                    name: bound.target.clone(),
                })
                .with_span(lowered.source_map.type_span(bound.target_ref)),
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
    table: &mut TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    if cancel.check().is_err() || table.has_constraint(reference.ty) {
        return;
    }
    // Every trait constraint retains its applied type arguments in its identity.
    let (name, applied) = match &lowered.module.type_ref(reference.ty).kind {
        TypeKind::Generic { name, .. } => (name, true),
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
        ty::resolve_named_type(name, context)
    };
    if applied && let TypeKind::Generic { args, .. } = &lowered.module.type_ref(reference.ty).kind {
        for argument in args {
            if table
                .type_ref(*argument)
                .is_some_and(|resolved| resolved.ty.is_unresolved())
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                        type_name: ty::display_type(&lowered.module, *argument),
                    })
                    .with_span(lowered.source_map.type_span(*argument)),
                );
            }
        }
    }
    // The two sealed numeric predicates are language bounds, not traits from
    // a source catalog. A binder or explicit declaration still shadows them.
    let standard = (resolved.target.is_none() && context.declarations.names.lookup(name).is_none())
        .then_some(match name.as_str() {
            "OrderedNumber" => Some(StandardTypeConstraint::OrderedNumber),
            "SignedNumber" => Some(StandardTypeConstraint::SignedNumber),
            _ => None,
        })
        .flatten();
    let target = standard
        .map(ConstraintTarget::Standard)
        .or(match &resolved.ty {
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
                Some(ConstraintTarget::Trait(instance.clone()))
            }
            _ => None,
        });
    let reason = if applied && !matches!(resolved.ty, TypeId::Trait(_)) {
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
                    trait_name: ty::display_type(&lowered.module, reference.ty),
                    reason,
                }
            } else {
                DiagnosticKind::UnknownTrait {
                    trait_name: ty::display_type(&lowered.module, reference.ty),
                }
            })
            .with_span(lowered.source_map.type_span(reference.ty)),
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

pub(super) fn implementation_bounds(
    implementation: &Impl,
    declarations: &Declarations,
    table: &TypeTable,
) -> super::GenericBounds {
    let mut result = parameter_bounds(&implementation.generic_params, declarations, table);
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
    params: &[GenericParam],
    declarations: &Declarations,
    table: &TypeTable,
) -> super::GenericBounds {
    params
        .iter()
        .filter_map(|param| {
            Some((
                TypeId::Generic(declarations.generic_type(param.id)?),
                param
                    .bounds
                    .iter()
                    .filter_map(|reference| table.constraint(reference.ty))
                    .collect(),
            ))
        })
        .collect()
}

pub fn type_satisfies_standard_constraint(
    ty: &TypeId,
    constraint: StandardTypeConstraint,
    bounds: &super::GenericBounds,
) -> bool {
    match constraint {
        StandardTypeConstraint::HashKey => {
            intrinsic_holds(StandardTrait::Eq, ty, None, bounds)
                && intrinsic_holds(StandardTrait::Hash, ty, None, bounds)
        }
        StandardTypeConstraint::Comparable => {
            intrinsic_holds(StandardTrait::PartialEq, ty, None, bounds)
        }
        _ if matches!(ty, TypeId::Generic(_) | TypeId::Projection { .. }) => bounds
            .get(ty)
            .is_some_and(|bounds| bounds.contains(&ConstraintTarget::Standard(constraint))),
        StandardTypeConstraint::OrderedNumber => surface::supports_ordering(ty, ty),
        StandardTypeConstraint::SignedNumber => surface::supports_unary_negation(ty),
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
        StandardTypeConstraint::HashKey => "key type must implement std::cmp::Eq + std::hash::Hash",
        StandardTypeConstraint::OrderedNumber => "type is not an ordered numeric type",
        StandardTypeConstraint::SignedNumber => "type is not a signed numeric type",
        StandardTypeConstraint::Comparable => "type does not have standard equality semantics",
    }
}
