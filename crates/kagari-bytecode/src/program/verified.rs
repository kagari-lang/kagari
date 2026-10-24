//! Immutable evidence for a resource-bounded, fully checked bytecode graph.
use crate::{
    artifact::{ArtifactValidationError, validate_program_resource_limits},
    program::{
        BytecodeProgram, ModuleRef, construction::UpdateCoverage, exports::ExportTarget, identity,
        verify_graph,
    },
    suspension::{AwaitLiveness, ProgramSuspensions},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};

use kagari_contract::updates::demands::Coverage;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct ProgramVerification {
    pub suspensions: ProgramSuspensions,
    pub exports: HashMap<DefinitionId, ExportTarget>,
    pub update_coverage: UpdateCoverage,
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
    /// Validate already scoped records and retain their exact immutable identities.
    ///
    /// Compiler output has already collected implicit declaration names. This
    /// entrypoint checks the same resource and graph obligations as editable
    /// input; possessing scoped metadata alone grants no executable authority.
    pub fn from_metadata(
        metadata: DefinitionMetadata<BytecodeProgram>,
        cancel: &CancellationToken,
    ) -> Result<Self, ArtifactValidationError> {
        cancel
            .check()
            .map_err(|_| ArtifactValidationError::Identity(DefinitionMappingError::Cancelled))?;
        validate_program_resource_limits(metadata.records(), metadata.definitions(), cancel)?;
        let verification = verify_graph(metadata.records(), metadata.definitions(), cancel)
            .map_err(ArtifactValidationError::Bytecode)?;
        cancel
            .check()
            .map_err(|_| ArtifactValidationError::Identity(DefinitionMappingError::Cancelled))?;
        Ok(Self {
            metadata,
            verification,
        })
    }

    pub fn new(
        program: BytecodeProgram,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<Self, ArtifactValidationError> {
        validate_program_resource_limits(&program, &context.snapshot(), cancel)?;
        let definitions =
            identity::collect(&program.modules.iter().collect::<Vec<_>>(), context, cancel)
                .map_err(ArtifactValidationError::Bytecode)?;
        let verification = verify_graph(&program, &definitions, cancel)
            .map_err(ArtifactValidationError::Bytecode)?;
        Self::from_verified(program, verification, definitions, cancel)
    }

    // Only bytecode-owned validation may retain evidence without repeating it.
    pub(crate) fn from_verified(
        program: BytecodeProgram,
        verification: ProgramVerification,
        definitions: DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, ArtifactValidationError> {
        let metadata = DefinitionMetadata::checked(definitions, program, cancel)
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

    /// Checked candidate products. Installed native callback evidence belongs to Runtime.
    pub fn update_coverage(&self) -> Coverage<'_> {
        Coverage {
            evidence: &self.program().update_evidence,
            methods: &self.verification.update_coverage.methods,
            constructions: &self.verification.update_coverage.constructions,
            layouts: &self.verification.update_coverage.layouts,
            natives: &[],
        }
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
