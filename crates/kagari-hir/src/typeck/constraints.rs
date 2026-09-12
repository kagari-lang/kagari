use std::collections::HashMap;

use kagari_common::{Diagnostic, DiagnosticKind, cancellation::CancellationToken};
use smallvec::SmallVec;

use crate::{builtin::surface, hir, lower::LoweredModule, types::TypeId};

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
            resolve_constraint(lowered, reference, context, table, diagnostics, cancel);
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
) {
    if cancel.check().is_err() || table.has_constraint(reference.ty) {
        return;
    }
    // Generic trait applications require concrete type arguments and implementation
    // tables. Retain their argument facts, but never silently erase the arguments.
    let applied =
        if let hir::TypeKind::Generic { args, .. } = &lowered.module.type_ref(reference.ty).kind {
            for arg in args {
                resolve_type_in(&lowered.module, *arg, context, table, cancel);
            }
            true
        } else {
            false
        };
    let target = surface::standard_constraint(&reference.name)
        .map(ConstraintTarget::Standard)
        .or_else(|| {
            lowered
                .module
                .traits
                .iter()
                .find(|item| item.name == reference.name)
                .map(|item| ConstraintTarget::Trait(item.id))
        });
    if let Some(ConstraintTarget::Trait(id)) = target {
        table.insert_type_ref(
            reference.ty,
            ResolvedTypeRef {
                ty: if applied {
                    TypeId::Error
                } else {
                    context
                        .declarations
                        .definition(crate::resolver::ResolvedName::Trait(id))
                        .cloned()
                        .map(TypeId::Trait)
                        .unwrap_or(TypeId::Error)
                },
                target: Some(TypeTarget::Trait(id)),
            },
        );
    }
    let target = target.filter(|_| !applied);
    table.insert_constraint(reference.ty, target);
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
            Diagnostic::error(DiagnosticKind::UnknownTrait {
                trait_name: super::ty::display_type(&lowered.module, reference.ty),
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
    let mut result = function
        .generic_params
        .iter()
        .filter_map(|param| {
            Some((
                declarations.generic_type(param.id)?,
                param
                    .bounds
                    .iter()
                    .filter_map(|reference| table.constraint(reference.ty))
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<HashMap<_, _>>();
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
