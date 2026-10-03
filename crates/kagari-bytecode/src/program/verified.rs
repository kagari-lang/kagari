//! Immutable evidence for a resource-bounded, fully checked bytecode graph.
use crate::{
    artifact::{ArtifactValidationError, validate_program_resource_limits},
    program::{BytecodeProgram, verify_program},
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
    program: BytecodeProgram,
}

impl VerifiedBytecodeProgram {
    pub fn new(program: BytecodeProgram) -> Result<Self, ArtifactValidationError> {
        validate_program_resource_limits(&program)?;
        verify_program(&program).map_err(ArtifactValidationError::Bytecode)?;
        Ok(Self::from_verified(program))
    }

    // Only bytecode-owned validation may retain evidence without repeating it.
    pub(crate) fn from_verified(program: BytecodeProgram) -> Self {
        Self { program }
    }

    pub fn program(&self) -> &BytecodeProgram {
        &self.program
    }

    pub fn into_unverified(self) -> BytecodeProgram {
        self.program
    }
}
