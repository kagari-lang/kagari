//! Match engine-owned implementation descriptors to portable type applications.
use crate::standard::application::StandardArguments;
use crate::standard::declarations::ApiImplementation;
use crate::standard::traits::{self, StandardTrait};
use crate::types::substitution::{TypeSubstitution, TypeTransformError};
use crate::types::{AbiType, ConstraintAbi, GenericBoundAbi, NominalAbiType};
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::associated_type_id;

/// Bind the receiver without requiring parameters that occur only in trait inputs.
/// Readonly storage is admitted only by the declared readonly capabilities.
pub fn match_receiver(
    declaration: &ApiImplementation,
    receiver: &AbiType,
    cancel: &CancellationToken,
) -> Result<Option<StandardArguments>, TypeTransformError> {
    let mut bindings = StandardArguments::new(declaration.generics);
    bindings.bind(&declaration.target, receiver, cancel)?;
    bindings.bind_receiver(receiver, cancel)?;
    Ok(receiver_matches(declaration, receiver, &bindings, cancel)?.then_some(bindings))
}

/// Match a trait's inputs; associated outputs and bounds remain explicit proof
/// obligations and are never inferred from the requested associated bindings.
pub fn match_application(
    declaration: &ApiImplementation,
    interface: &NominalAbiType,
    receiver: &AbiType,
    cancel: &CancellationToken,
) -> Result<Option<StandardArguments>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    if declaration.trait_declaration().item.identity() != interface.declaration
        || declaration.trait_arguments.len() != interface.arguments.len()
    {
        return Ok(None);
    }
    let Some(mut bindings) = match_receiver(declaration, receiver, cancel)? else {
        return Ok(None);
    };
    for (template, actual) in declaration.trait_arguments.iter().zip(&interface.arguments) {
        bindings.bind(template, actual, cancel)?;
    }
    if !receiver_matches(declaration, receiver, &bindings, cancel)? {
        return Ok(None);
    }
    for (template, actual) in declaration.trait_arguments.iter().zip(&interface.arguments) {
        if bindings.resolve(template, cancel)?.as_ref() != Some(actual) {
            return Ok(None);
        }
    }
    let applied = applied_contract(declaration, &bindings, cancel)?;
    Ok(interface
        .associated_types
        .iter()
        .all(|(member, value)| applied.associated_types.get(member) == Some(value))
        .then_some(bindings))
}

fn receiver_matches(
    declaration: &ApiImplementation,
    receiver: &AbiType,
    bindings: &StandardArguments,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    Ok(bindings
        .resolve(&declaration.target, cancel)?
        .is_some_and(|target| {
            target == *receiver
                || matches!(declaration.interface, "Iterable" | "List" | "Map" | "Set")
                    && target.can_weaken_to(receiver)
        }))
}

pub fn applied_contract(
    declaration: &ApiImplementation,
    bindings: &StandardArguments,
    cancel: &CancellationToken,
) -> Result<NominalAbiType, TypeTransformError> {
    let owner = traits::identity(
        StandardTrait::from_name(declaration.interface)
            .ok_or(TypeTransformError::InvalidContract)?,
    );
    let applied = NominalAbiType {
        declaration: owner.clone(),
        arguments: declaration
            .trait_arguments
            .iter()
            .map(|template| {
                bindings
                    .resolve(template, cancel)?
                    .ok_or(TypeTransformError::InvalidContract)
            })
            .collect::<Result<_, _>>()?,
        associated_types: declaration
            .associated_types
            .iter()
            .map(|(member, template)| {
                Ok((
                    associated_type_id(
                        &owner,
                        member
                            .path
                            .last()
                            .ok_or(TypeTransformError::InvalidContract)?
                            .1,
                    ),
                    bindings
                        .resolve(template, cancel)?
                        .ok_or(TypeTransformError::InvalidContract)?,
                ))
            })
            .collect::<Result<_, TypeTransformError>>()?,
    };
    TypeSubstitution::default().apply_nominal(&applied, cancel)
}

/// A matching descriptor is not a proof: its instantiated bounds must also hold.
pub fn requirements(
    declaration: &ApiImplementation,
    bindings: &StandardArguments,
    cancel: &CancellationToken,
) -> Result<Vec<GenericBoundAbi>, TypeTransformError> {
    declaration
        .bounds
        .iter()
        .map(|(target, constraints)| {
            Ok(GenericBoundAbi {
                ty: bindings
                    .resolve(target, cancel)?
                    .ok_or(TypeTransformError::InvalidContract)?,
                constraints: constraints
                    .iter()
                    .map(|constraint| {
                        bindings
                            .resolve_bound(constraint, cancel)
                            .map(ConstraintAbi::Trait)
                    })
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
