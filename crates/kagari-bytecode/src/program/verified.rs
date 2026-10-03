//! Immutable evidence for a resource-bounded, fully checked bytecode graph.
use crate::{
    artifact::{ArtifactValidationError, validate_program_resource_limits},
    program::{BytecodeProgram, verify_program},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        metadata::{DefinitionMetadata, PortableMetadata, scope_record},
        table::wire::PortableDefinitionRef,
        table::{DefinitionId, DefinitionTable},
    },
};

/// Verification belongs to these exact owned bytes, never a serialized flag or hash.
/// Clones preserve the evidence; extracting mutable code discards it.
///
/// ```compile_fail
/// use kagari_bytecode::program::verified::VerifiedBytecodeProgram;
/// fn mutate(program: &mut VerifiedBytecodeProgram) {
///     program.program().modules.clear();
/// }
/// ```
///
/// ```compile_fail
/// use kagari_bytecode::program::verified::VerifiedBytecodeProgram;
/// fn decode(bytes: &[u8]) -> VerifiedBytecodeProgram {
///     bincode::deserialize(bytes).unwrap()
/// }
/// ```
#[derive(Debug, Clone)]
pub struct VerifiedBytecodeProgram {
    metadata: DefinitionMetadata<BytecodeProgram<DefinitionId>>,
}

impl VerifiedBytecodeProgram {
    pub fn new(program: BytecodeProgram) -> Result<Self, ArtifactValidationError> {
        validate_program_resource_limits(&program)?;
        verify_program(&program).map_err(ArtifactValidationError::Bytecode)?;
        Self::from_verified(program)
    }

    // Only bytecode-owned validation may retain evidence without repeating it.
    pub(crate) fn from_verified(program: BytecodeProgram) -> Result<Self, ArtifactValidationError> {
        let metadata = scope_record(&program, &CancellationToken::default())
            .map_err(ArtifactValidationError::Identity)?;
        Ok(Self { metadata })
    }

    pub fn program(&self) -> &BytecodeProgram<DefinitionId> {
        self.metadata.records()
    }

    pub fn definitions(&self) -> &DefinitionTable {
        self.metadata.definitions()
    }

    pub fn portable_projection(
        &self,
        cancel: &CancellationToken,
    ) -> Result<PortableMetadata<BytecodeProgram<PortableDefinitionRef>>, ArtifactValidationError>
    {
        self.metadata
            .to_portable(cancel)
            .map_err(ArtifactValidationError::Identity)
    }

    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<BytecodeProgram, ArtifactValidationError> {
        self.metadata
            .to_paths(cancel)
            .map_err(ArtifactValidationError::Identity)
    }

    pub fn into_unverified(self) -> BytecodeProgram {
        self.to_unverified(&CancellationToken::default())
            .expect("verified bytecode retains its bounded identity scope")
    }
}
