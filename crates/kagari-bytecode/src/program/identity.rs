//! Collect implicit declaration names after checking the complete input snapshot.
use crate::{module::BytecodeModule, verifier::BytecodeVerificationError};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, mapping::DefinitionRecord, table::DefinitionTable},
};
use kagari_contract::types::identity;

/// Existing references must belong to the input prefix before any names are added.
/// Collected names assign identity only; module and linked verifiers establish ABI.
pub fn collect(
    modules: &[&BytecodeModule],
    context: &DefinitionContext,
) -> Result<DefinitionTable, BytecodeVerificationError> {
    let cancel = CancellationToken::default();
    let input = context.snapshot();
    for module in modules {
        module
            .visit_definitions(&mut |id| Ok(input.resolve(*id).map(|_| ())?), &cancel)
            .map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    }
    for module in modules {
        identity::collect(
            &module.identity,
            &module.public_items,
            &module.trait_contracts,
            context,
            &cancel,
        )
        .map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    }
    Ok(context.snapshot())
}
