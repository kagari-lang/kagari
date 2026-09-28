//! Applied trait ancestry over portable declaration contracts.
use crate::types::substitution::{
    TypeSubstitution, TypeTransformError, resolve_associated_outputs,
};
use crate::types::{AbiType, NominalAbiType, TraitAbi, standard_trait_contract};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};
use std::collections::HashSet;

const MAX_TRAITS: usize = 4_096;
const MAX_PATH: usize = 64;
const MAX_EDGES: usize = 100_000;

/// Preserve applied arguments and associated bindings while walking supertraits.
/// Declaration cycles fail even when they change arguments. Diamond paths to the
/// same application are deduplicated; distinct applications retain their identity.
pub fn trait_closure<'a>(
    interface: &NominalAbiType,
    receiver: &AbiType,
    cancel: &CancellationToken,
    lookup: &impl Fn(&DefinitionId) -> Option<&'a TraitAbi>,
) -> Result<Vec<NominalAbiType>, TypeTransformError> {
    let empty = TypeSubstitution::default();
    let root = empty.apply_nominal(interface, cancel)?;
    let receiver = empty.apply(receiver, cancel)?;
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = vec![(root, Vec::<DefinitionId>::new())];
    let mut remaining = MAX_EDGES;
    while let Some((applied, mut path)) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if path.contains(&applied.declaration) || path.len() >= MAX_PATH || remaining == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        remaining -= 1;
        if !seen.insert(applied.clone()) {
            continue;
        }
        if result.len() >= MAX_TRAITS {
            return Err(TypeTransformError::LimitExceeded);
        }
        let contract = standard_trait_contract(&applied.declaration)
            .or_else(|| lookup(&applied.declaration))
            .ok_or(TypeTransformError::InvalidContract)?;
        if contract.generic_params.len() != applied.arguments.len() {
            return Err(TypeTransformError::InvalidContract);
        }
        if contract.supertraits.len() > remaining
            || pending.len() > remaining - contract.supertraits.len()
        {
            return Err(TypeTransformError::LimitExceeded);
        }
        let mut parameters = TypeSubstitution::default();
        for (parameter, argument) in contract.generic_params.iter().zip(&applied.arguments) {
            parameters.bind(&parameter.owner, parameter.position, argument);
        }
        let mut receivers = TypeSubstitution::default();
        receivers.bind_receiver(&applied.declaration, &receiver);
        path.push(applied.declaration.clone());
        for parent in contract.supertraits.iter().rev() {
            let parent = AbiType::Trait(empty.apply_nominal(parent, cancel)?);
            let parent = resolve_associated_outputs(&parent, &applied, cancel)?;
            let parent = receivers.apply(&parent, cancel)?;
            let parent = parameters.apply(&parent, cancel)?;
            let AbiType::Trait(parent) = parent else {
                return Err(TypeTransformError::InvalidContract);
            };
            pending.push((parent, path.clone()));
        }
        result.push(applied);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
