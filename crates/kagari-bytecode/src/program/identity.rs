//! Collect implicit declaration names after checking the complete input snapshot.
use crate::{module::BytecodeModule, verifier::BytecodeVerificationError};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext, mapping::DefinitionMappingError, scoped::DefinitionReferences,
        table::DefinitionTable,
    },
};
use kagari_contract::types::identity;

/// Existing references must belong to the input prefix before any names are added.
/// Collected names assign identity only; module and linked verifiers establish ABI.
pub fn collect(
    modules: &[&BytecodeModule],
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<DefinitionTable, BytecodeVerificationError> {
    let input = context.snapshot();
    validate(modules, &input, cancel)?;
    for module in modules {
        identity::collect(
            &module.identity,
            &module.public_items,
            &module.trait_contracts,
            context,
            cancel,
        )
        .map_err(|error| match error {
            DefinitionMappingError::Cancelled => BytecodeVerificationError::Cancelled,
            _ => BytecodeVerificationError::InvalidPublicAbi,
        })?;
    }
    Ok(context.snapshot())
}

pub(crate) fn validate(
    modules: &[&BytecodeModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), BytecodeVerificationError> {
    for module in modules {
        module
            .visit_definition_ids(&mut |id| Ok(definitions.resolve(*id).map(|_| ())?), cancel)
            .map_err(|error| match error {
                DefinitionMappingError::Cancelled => BytecodeVerificationError::Cancelled,
                _ => BytecodeVerificationError::InvalidPublicAbi,
            })?;
    }
    Ok(())
}
