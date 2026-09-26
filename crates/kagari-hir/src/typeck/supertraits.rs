//! Validate inheritance after every module's declaration contracts are available.
use crate::{
    aggregates::AggregateCatalog,
    declarations::Declarations,
    lower::LoweredModule,
    types::{NominalType, TypeId},
};
use kagari_common::{Diagnostic, DiagnosticKind, cancellation::CancellationToken};

pub(crate) fn trait_supertrait_surface(
    module: &crate::hir::Module,
    item: &crate::hir::TraitDef,
    declarations: &Declarations,
    cancel: &CancellationToken,
) -> Vec<NominalType> {
    let mut table = super::TypeTable::default();
    item.supertraits
        .iter()
        .filter_map(|reference| {
            match super::ty::resolve_type_in(
                module,
                reference.ty,
                super::ty::TypeContext {
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
    table: &super::TypeTable,
    diagnostics: &mut crate::DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let identity = lowered.source.module_identity();
    for item in &lowered.module.traits {
        for reference in &item.supertraits {
            if matches!(
                table.constraint(reference.ty),
                Some(super::ConstraintTarget::Standard(_))
            ) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidTraitReference {
                        trait_name: super::ty::display_type(&lowered.module, reference.ty),
                        reason: "a supertrait must name a declared user trait",
                    })
                    .with_span(lowered.source_map.type_span(reference.ty)),
                );
            }
        }
    }
    for contract in catalog
        .traits()
        .filter(|contract| &contract.id.module == identity)
    {
        let applied = NominalType {
            declaration: contract.id.clone(),
            arguments: contract
                .generic_params
                .iter()
                .cloned()
                .map(TypeId::Generic)
                .collect(),
            associated_types: Default::default(),
        };
        let receiver = TypeId::SelfType(contract.id.clone());
        let mut assumptions = contract.bounds.clone();
        assumptions
            .entry(receiver.clone())
            .or_default()
            .push(super::ConstraintTarget::Trait(applied.clone()));
        if let Ok(assumptions) = catalog.expanded_bounds(&assumptions, cancel) {
            for parent in &contract.supertraits {
                super::applications::validate(
                    &TypeId::Trait(parent.clone()),
                    &assumptions,
                    (catalog, &declarations.hosts),
                    table,
                    contract.declaration.location.range,
                    diagnostics,
                    cancel,
                );
            }
        }
        if catalog
            .trait_closure(&applied, &TypeId::SelfType(contract.id.clone()), cancel)
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
    for implementation in catalog
        .implementations()
        .filter(|item| &item.id.module == identity)
    {
        if let Some(reason) = catalog.standard_override_error(implementation) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: implementation
                        .trait_type
                        .declaration
                        .path
                        .last()
                        .unwrap()
                        .name
                        .clone(),
                    type_name: implementation.for_type.display_name(),
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
            let available =
                catalog.intrinsic_implementation(&parent, &implementation.for_type, &bounds)
                    || declarations
                        .hosts
                        .implements(&parent, &implementation.for_type)
                    || matches!(
                        catalog.concrete_interface_implementation(
                            &parent,
                            &implementation.for_type,
                            &bounds,
                            100_000,
                            64,
                            cancel
                        ),
                        Ok(Some(_))
                    );
            if !available {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                        trait_name: implementation
                            .trait_type
                            .declaration
                            .path
                            .last()
                            .unwrap()
                            .name
                            .clone(),
                        type_name: implementation.for_type.display_name(),
                        reason: format!(
                            "required supertrait `{}` is not implemented under these bounds",
                            TypeId::Trait(parent).display_name()
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
    for host in declarations.hosts.type_declarations() {
        for implementation in &host.trait_implementations {
            if &implementation.trait_id.module != identity {
                continue;
            }
            let applied = crate::host::HostDeclarations::trait_type(implementation);
            let receiver = TypeId::Host(host.id.clone());
            let Ok(parents) = catalog.trait_closure(&applied, &receiver, cancel) else {
                continue;
            };
            for parent in parents.into_iter().skip(1) {
                if !declarations.hosts.implements(&parent, &receiver)
                    && catalog.implementation_count(&parent, &receiver) != 1
                {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                            trait_name: TypeId::Trait(applied.clone()).display_name(),
                            type_name: host.symbol.clone(),
                            reason: format!(
                                "missing host supertrait `{}`",
                                TypeId::Trait(parent).display_name()
                            ),
                        })
                        .with_span(kagari_common::Span::default()),
                    );
                }
            }
        }
    }
}
