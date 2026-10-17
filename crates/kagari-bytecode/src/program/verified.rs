//! Immutable evidence for a resource-bounded, fully checked bytecode graph.
use crate::{
    artifact::{ArtifactValidationError, validate_program_resource_limits},
    program::{BytecodeProgram, ModuleRef, exports::ExportTarget, identity, verify_graph},
    suspension::{AwaitLiveness, ProgramSuspensions},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct ProgramVerification {
    pub suspensions: ProgramSuspensions,
    pub exports: HashMap<DefinitionId, ExportTarget>,
}

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
    verification: ProgramVerification,
}

impl VerifiedBytecodeProgram {
    pub fn new(
        program: BytecodeProgram,
        context: &DefinitionContext,
    ) -> Result<Self, ArtifactValidationError> {
        validate_program_resource_limits(&program, &context.snapshot())?;
        let definitions = identity::collect(&program.modules.iter().collect::<Vec<_>>(), context)
            .map_err(ArtifactValidationError::Bytecode)?;
        let verification =
            verify_graph(&program, &definitions).map_err(ArtifactValidationError::Bytecode)?;
        Self::from_verified(program, verification, definitions)
    }

    // Only bytecode-owned validation may retain evidence without repeating it.
    pub(crate) fn from_verified(
        program: BytecodeProgram,
        verification: ProgramVerification,
        definitions: DefinitionTable,
    ) -> Result<Self, ArtifactValidationError> {
        let metadata =
            DefinitionMetadata::checked(definitions, program, &CancellationToken::default())
                .map_err(ArtifactValidationError::Identity)?;
        Ok(Self {
            metadata,
            verification,
        })
    }

    pub fn suspensions(&self, module: ModuleRef) -> Option<&[Vec<AwaitLiveness>]> {
        self.verification
            .suspensions
            .get(module.index())
            .map(Vec::as_slice)
    }

    /// Exact slots for retained public leaves and the separate entry in this Program.
    /// IDs belong to `definitions()`; these records cannot verify a different Program.
    pub fn export_targets(&self) -> &HashMap<DefinitionId, ExportTarget> {
        &self.verification.exports
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
