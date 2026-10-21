//! Declaration-derived capabilities for constructing interface values.
use crate::callable::{
    bounds::expanded_bounds, construction::evidence::ConstructionRequirement,
    interface::dynamic_interface_valid,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::TraitDef,
    ty::{
        GenericBound, GenericParam, Ty, inheritance::trait_closure,
        substitution::TypeTransformError,
    },
};
use std::cmp::Ordering;

/// Enumerate dynamic capabilities independently of the number of trait methods.
/// Missing non-generic outputs retain their receiver projection. Static-only
/// bounds still supply method operations, but do not supply these capabilities.
/// Returns errors for malformed declarations, traversal limits and cancellation.
pub fn required_constructions<'a>(
    bounds: &[GenericBound],
    parameters: &[GenericParam],
    lookup: &impl Fn(&DefinitionId) -> Option<&'a TraitDef>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<Vec<ConstructionRequirement>, TypeTransformError> {
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
                if !dynamic_interface_valid(parameters, &interface, lookup, definitions, cancel)? {
                    continue;
                }
                let required = ConstructionRequirement {
                    receiver: bound.ty.clone(),
                    interface,
                };
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
