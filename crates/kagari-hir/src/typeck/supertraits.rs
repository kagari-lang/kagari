//! Validate inheritance after every module's declaration contracts are available.

use crate::{
    DiagnosticBuffer,
    aggregates::{AggregateCatalog, implementations::ImplementationSearchLimits},
    declarations::Declarations,
    hir::item::{Module, behavior::TraitDef},
    lower::LoweredModule,
    typeck::{
        applications,
        table::TypeTable,
        ty::{self, TypeContext},
    },
    types::{NominalType, TypeId},
};

use {
    kagari_common::cancellation::CancellationToken,
    kagari_source::diagnostic::{Diagnostic, DiagnosticKind},
};

pub(crate) fn trait_supertrait_surface(
    module: &Module,
    item: &TraitDef,
    declarations: &Declarations,
    cancel: &CancellationToken,
) -> Vec<NominalType> {
    let mut table = TypeTable::default();
    item.supertraits
        .iter()
        .filter_map(|reference| {
            match ty::resolve_type_in(
                module,
                reference.ty,
                TypeContext {
                    declarations,
                    generics: &item.generic_params,
                    self_type: Some(item.id),
                    implementation: None,
                },
                &mut table,
                cancel,
            ) {
                TypeId::Trait(parent) => Some(parent),
                _ => None,
            }
        })
        .collect()
}

pub(crate) fn validate(
    lowered: &LoweredModule,
    declarations: &Declarations,
    catalog: &AggregateCatalog,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let identity = lowered.source.module_identity();
    for contract in catalog.traits().filter(|contract| {
        declarations
            .definitions()
            .resolve(contract.id)
            .expect("scoped trait")
            .module()
            == identity
    }) {
        let applied = NominalType {
            declaration: contract.id,
            arguments: contract
                .generic_params
                .iter()
                .cloned()
                .map(TypeId::Generic)
                .collect(),
            associated_types: Default::default(),
        };
        let receiver = TypeId::SelfType(contract.id);
        let mut assumptions = contract.bounds.clone();
        assumptions
            .entry(receiver.clone())
            .or_default()
            .push(applied.clone());
        if let Ok(assumptions) = catalog.expanded_bounds(&assumptions, cancel) {
            for parent in &contract.supertraits {
                applications::validate(
                    applications::ApplicationContext {
                        definitions: declarations.definitions(),
                        catalog,
                        receiver: None,
                    },
                    &TypeId::Trait(parent.clone()),
                    &assumptions,
                    contract.declaration.location.range,
                    diagnostics,
                    cancel,
                );
            }
        }
        if catalog
            .trait_closure(&applied, &TypeId::SelfType(contract.id), cancel)
            .is_err()
            && cancel.check().is_ok()
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitReference {
                    trait_name: contract.declaration.name.clone(),
                    reason: "cyclic, unavailable or excessively deep supertrait graph",
                })
                .with_span(contract.declaration.location.range),
            );
        }
    }
    for implementation in catalog.implementations().filter(|item| {
        declarations
            .definitions()
            .resolve(item.id)
            .expect("scoped implementation")
            .module()
            == identity
    }) {
        if let Some(reason) =
            catalog.standard_override_error(declarations.definitions(), implementation)
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: declarations
                        .definitions()
                        .resolve(implementation.trait_type.declaration)
                        .expect("scoped trait")
                        .last()
                        .expect("trait identity")
                        .name
                        .to_owned(),
                    type_name: implementation
                        .for_type
                        .display_name(declarations.definitions()),
                    reason: reason.into(),
                })
                .with_span(
                    lowered
                        .module
                        .impls
                        .iter()
                        .find(|item| {
                            declarations.impl_identity(item.id) == Some(&implementation.id)
                        })
                        .map(|item| lowered.source_map.impl_span(item.id))
                        .unwrap_or_default(),
                ),
            );
        }
        let Ok(parents) =
            catalog.trait_closure(&implementation.trait_type, &implementation.for_type, cancel)
        else {
            continue;
        };
        let Ok(bounds) = catalog.expanded_bounds(&implementation.bounds, cancel) else {
            continue;
        };
        for parent in parents.into_iter().skip(1) {
            let available = catalog.intrinsic_implementation(
                declarations.definitions(),
                &parent,
                &implementation.for_type,
                &bounds,
            ) || matches!(
                catalog.concrete_interface_implementation(
                    declarations.definitions(),
                    &parent,
                    &implementation.for_type,
                    &bounds,
                    ImplementationSearchLimits {
                        max_checks: 100_000,
                        max_depth: 64
                    },
                    cancel
                ),
                Ok(Some(_))
            );
            if !available {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                        trait_name: declarations
                            .definitions()
                            .resolve(implementation.trait_type.declaration)
                            .expect("scoped trait")
                            .last()
                            .expect("trait identity")
                            .name
                            .to_owned(),
                        type_name: implementation
                            .for_type
                            .display_name(declarations.definitions()),
                        reason: format!(
                            "required supertrait `{}` is not implemented under these bounds",
                            TypeId::Trait(parent).display_name(declarations.definitions())
                        ),
                    })
                    .with_span(
                        lowered
                            .module
                            .impls
                            .iter()
                            .find(|item| {
                                declarations.impl_identity(item.id) == Some(&implementation.id)
                            })
                            .map(|item| lowered.source_map.impl_span(item.id))
                            .unwrap_or_default(),
                    ),
                );
            }
        }
    }
}
