//! Immutable evidence for a resource-bounded, fully checked bytecode graph.
use crate::{
    artifact::{ArtifactValidationError, validate_program_resource_limits},
    program::{BytecodeProgram, ModuleRef, identity, verify_with_suspensions},
    suspension::{AwaitLiveness, ProgramSuspensions},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionTable},
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
    metadata: DefinitionMetadata<BytecodeProgram>,
    suspensions: ProgramSuspensions,
}

impl VerifiedBytecodeProgram {
    pub fn new(
        program: BytecodeProgram,
        context: &DefinitionContext,
    ) -> Result<Self, ArtifactValidationError> {
        validate_program_resource_limits(&program, &context.snapshot())?;
        let definitions = identity::collect(&program.modules.iter().collect::<Vec<_>>(), context)
            .map_err(ArtifactValidationError::Bytecode)?;
        let suspensions = verify_with_suspensions(&program, &definitions)
            .map_err(ArtifactValidationError::Bytecode)?;
        Self::from_verified(program, suspensions, definitions)
    }

    // Only bytecode-owned validation may retain evidence without repeating it.
    pub(crate) fn from_verified(
        program: BytecodeProgram,
        suspensions: ProgramSuspensions,
        definitions: DefinitionTable,
    ) -> Result<Self, ArtifactValidationError> {
        let metadata =
            DefinitionMetadata::checked(definitions, program, &CancellationToken::default())
                .map_err(ArtifactValidationError::Identity)?;
        Ok(Self {
            metadata,
            suspensions,
        })
    }

    pub fn suspensions(&self, module: ModuleRef) -> Option<&[Vec<AwaitLiveness>]> {
        self.suspensions.get(module.index()).map(Vec::as_slice)
    }

    pub fn program(&self) -> &BytecodeProgram {
        self.metadata.records()
    }

    pub fn definitions(&self) -> &DefinitionTable {
        self.metadata.definitions()
    }

    /// Copy editable records with their retained identity scope, without the seal.
    pub fn to_unverified(&self) -> DefinitionMetadata<BytecodeProgram> {
        self.metadata.clone()
    }

    /// Extract editable records with their retained identity scope, without the seal.
    pub fn into_unverified(self) -> DefinitionMetadata<BytecodeProgram> {
        self.metadata
    }
}
