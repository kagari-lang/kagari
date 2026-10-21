//! Retained identity scopes stay separate from MIR verification evidence.
use crate::{
    function::MirModule,
    verify::{MirVerificationError, MirVerificationErrorKind, VerifiedMirModule},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError},
        metadata::DefinitionMetadata,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionTable, DefinitionTableError},
    },
};
use kagari_contract::types::identity;

pub(crate) fn mapping_error(cause: DefinitionMappingError) -> MirVerificationError {
    MirVerificationError {
        function: None,
        block: None,
        instruction: None,
        span: None,
        kind: match cause {
            DefinitionMappingError::Cancelled => MirVerificationErrorKind::Cancelled,
            DefinitionMappingError::Identity(DefinitionTableError::PortableLimit) => {
                MirVerificationErrorKind::Limit {
                    resource: "definitions",
                    limit: 1_000_000,
                }
            }
            DefinitionMappingError::LimitExceeded => MirVerificationErrorKind::Limit {
                resource: "identity mapping",
                limit: 1_000_000,
            },
            _ => MirVerificationErrorKind::InvalidInstance,
        },
    }
}

impl VerifiedMirModule {
    /// The immutable scope retained by this verification seal.
    pub fn definitions(&self) -> &DefinitionTable {
        self.metadata.definitions()
    }

    /// Copy editable records and their identity scope, without verification evidence.
    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<MirModule>, MirVerificationError> {
        // Use the record traversal to keep large copies cancellable. Identity
        // values remain in this exact scope throughout the copy.
        let records = self
            .metadata
            .records()
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| Ok(*id),
                self.definitions(),
                cancel,
            ))
            .map_err(mapping_error)?;
        DefinitionMetadata::checked(self.definitions().clone(), records, cancel)
            .map_err(mapping_error)
    }

    /// Transfer editable records and their scope, discarding the verification seal.
    pub fn into_unverified(self) -> DefinitionMetadata<MirModule> {
        self.metadata
    }
}

/// Collect ABI-owned names only after every input reference has been validated
/// against the context's original prefix. Names confer no verification evidence.
pub(crate) fn collect(
    modules: &[MirModule],
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<DefinitionTable, DefinitionMappingError> {
    let input = context.snapshot();
    for module in modules {
        module.visit_definition_ids(&mut |id| Ok(input.resolve(*id).map(|_| ())?), cancel)?;
    }
    for module in modules {
        identity::collect(
            &module.identity,
            &module.abi.public_items,
            &module.abi.trait_contracts,
            context,
            cancel,
        )?;
    }
    Ok(context.snapshot())
}
