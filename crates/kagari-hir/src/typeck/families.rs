//! Check type-family contracts under their own declaration-owned binders.
use super::{ConstraintTarget, ModuleSignatures};
use crate::{
    aggregates::AggregateCatalog,
    declarations::Declarations,
    lower::LoweredModule,
    types::{TypeId, TypeSubstitution},
};
use kagari_common::{Diagnostic, DiagnosticKind, cancellation::CancellationToken};

pub(super) fn validate(
    lowered: &LoweredModule,
    declarations: &Declarations,
    signatures: &ModuleSignatures,
    catalog: &AggregateCatalog,
    diagnostics: &mut crate::DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    for item in &lowered.module.impls {
        if cancel.check().is_err() {
            return;
        }
        let Some(implementation) = declarations
            .impl_identity(item.id)
            .and_then(|id| catalog.implementation_signature(id))
        else {
            continue;
        };
        let Some(contract) = catalog.trait_(&implementation.trait_type.declaration) else {
            continue;
        };
        for (member, family) in &implementation.associated_type_families {
            let Some(inputs) = contract.associated_type_parameters.get(member) else {
                continue;
            };
            let mut substitution: TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(implementation.trait_type.arguments.iter().cloned())
                .chain(
                    inputs.parameters.iter().cloned().zip(
                        family
                            .inputs
                            .parameters
                            .iter()
                            .cloned()
                            .map(TypeId::Generic),
                    ),
                )
                .collect();
            substitution.insert_receiver(contract.id.clone(), implementation.for_type.clone());
            let normalize_bound = |constraint: &ConstraintTarget| match constraint {
                ConstraintTarget::Standard(value) => ConstraintTarget::Standard(*value),
                ConstraintTarget::Trait(value) => {
                    ConstraintTarget::Trait(value.instantiate(&substitution))
                }
            };
            let required = inputs
                .bounds
                .iter()
                .map(|(target, constraints)| {
                    (
                        target.instantiate(&substitution),
                        constraints.iter().map(normalize_bound).collect::<Vec<_>>(),
                    )
                })
                .collect::<super::GenericBounds>();
            if family.inputs.bounds.iter().any(|(target, provided)| {
                required
                    .get(target)
                    .is_none_or(|required| provided.iter().any(|bound| !required.contains(bound)))
            }) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                        name: member.path.last().expect("member").name.clone(),
                        reason:
                            "generic associated type implementation strengthens the trait input bounds"
                                .into(),
                    })
                    .with_span(lowered.source_map.impl_span(item.id)),
                );
            }
            let mut available = implementation.bounds.clone();
            for (target, bounds) in required.into_iter().chain(family.inputs.bounds.clone()) {
                let entry = available.entry(target).or_default();
                for bound in bounds {
                    if !entry.contains(&bound) {
                        entry.push(bound);
                    }
                }
            }
            let available = catalog
                .expanded_bounds(&available, cancel)
                .unwrap_or(available);
            super::applications::validate(
                &family.value,
                &available,
                (catalog, &declarations.hosts),
                signatures.type_table(),
                lowered.source_map.impl_span(item.id),
                diagnostics,
                cancel,
            );
            let output = catalog.normalize_type(&family.value);
            if output.is_unresolved() {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                        name: member.path.last().expect("member").name.clone(),
                        reason: "generic associated type recursively depends on itself".into(),
                    })
                    .with_span(lowered.source_map.impl_span(item.id)),
                );
            }
            for required in contract.associated_types.get(member).into_iter().flatten() {
                let satisfies = match normalize_bound(required) {
                    ConstraintTarget::Standard(required) => super::type_satisfies_standard_constraint(&output, required, &available),
                    ConstraintTarget::Trait(required) => {
                        available.get(&output).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(actual) if actual.satisfies(&required))))
                            || catalog.concrete_interface_implementation(&required, &output, &available, 100_000, 64, cancel).ok().flatten().is_some()
                    }
                };
                if !satisfies {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                            name: member.path.last().expect("member").name.clone(),
                            reason:
                                "generic associated type output does not satisfy its declared bound"
                                    .into(),
                        })
                        .with_span(lowered.source_map.impl_span(item.id)),
                    );
                }
            }
        }
    }
}
