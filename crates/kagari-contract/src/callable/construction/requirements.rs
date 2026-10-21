//! Declaration-derived capabilities for constructing interface values.
use crate::{
    callable::{
        bounds::expanded_bounds,
        construction::evidence::{ConstructionRequirement, ConstructionWitness},
        interface::dynamic_interface_valid,
    },
    types::proofs::ProofCatalog,
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use kagari_types::ty::{
    GenericBound, GenericParam, Ty, inheritance::trait_closure, substitution::TypeTransformError,
};
use std::cmp::Ordering;

/// Enumerate dynamic capabilities independently of the number of trait methods.
/// Missing non-generic outputs are normalized from their receiver projection.
/// Symbolic receivers retain that projection. Static-only
/// bounds still supply method operations, but do not supply these capabilities.
/// Returns errors for malformed declarations, traversal limits and cancellation.
pub fn required_constructions(
    bounds: &[GenericBound],
    parameters: &[GenericParam],
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<Vec<ConstructionRequirement>, TypeTransformError> {
    let lookup = &|id: &DefinitionId| catalog.trait_contract(id);
    let definitions = catalog.definitions();
    let expanded = expanded_bounds(bounds, lookup, cancel)?;
    let mut requirements = Vec::new();
    for bound in &expanded {
        for interface in &bound.constraints {
            for mut interface in trait_closure(interface, &bound.ty, cancel, lookup)? {
                cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
                let contract =
                    lookup(&interface.declaration).ok_or(TypeTransformError::InvalidContract)?;
                // Projection identity uses the declared application, before filling
                // outputs, so it cannot contain a self-referential output map.
                let declared = interface.clone();
                for member in &contract.associated_types {
                    if member.generic_params.is_empty() {
                        interface
                            .associated_types
                            .entry(member.declaration)
                            .or_insert_with(|| Ty::Projection {
                                receiver: Box::new(bound.ty.clone()),
                                interface: Box::new(declared.clone()),
                                member: member.declaration,
                                arguments: vec![],
                            });
                    }
                }
                let required = ConstructionRequirement {
                    receiver: bound.ty.clone(),
                    interface,
                }
                .normalize(catalog, cancel)?;
                // Closed associated projections must resolve before dynamic eligibility.
                // Otherwise a methodless provider loses its capability at application.
                if !dynamic_interface_valid(
                    parameters,
                    &required.interface,
                    lookup,
                    definitions,
                    cancel,
                )? {
                    continue;
                }
                if !requirements.contains(&required) {
                    requirements.push(required);
                    if requirements.len() > 4096 {
                        return Err(TypeTransformError::LimitExceeded);
                    }
                }
            }
        }
    }
    let mut failure = None;
    requirements.sort_by(|left, right| {
        let order = left
            .receiver
            .canonical_cmp(&right.receiver, definitions, cancel)
            .and_then(|order| {
                if order.is_eq() {
                    Ty::Trait(left.interface.clone()).canonical_cmp(
                        &Ty::Trait(right.interface.clone()),
                        definitions,
                        cancel,
                    )
                } else {
                    Ok(order)
                }
            });
        match order {
            Ok(order) => order,
            Err(error) => {
                failure = Some(error);
                Ordering::Equal
            }
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(requirements)
}

/// Check a complete, unique construction list against the callee's applied bounds.
/// Normalization may merge capabilities; it cannot create an additional bound.
pub fn constructions_valid(
    witnesses: &[ConstructionWitness],
    bounds: &[GenericBound],
    catalog: &ProofCatalog<'_>,
    parameters: &[GenericParam],
    assumptions: &[GenericBound],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    if witnesses.len() > 4096 {
        return Ok(false);
    }
    let required = required_constructions(bounds, parameters, catalog, cancel)?;
    if required.len() != witnesses.len()
        || required.iter().any(|required| {
            witnesses
                .iter()
                .filter(|witness| &witness.requirement == required)
                .count()
                != 1
        })
    {
        return Ok(false);
    }
    for witness in witnesses {
        if !witness.valid(catalog, parameters, assumptions, cancel)? {
            return Ok(false);
        }
    }
    Ok(true)
}
