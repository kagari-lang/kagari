//! Shared immutable executable input, independent of runtime instances.
mod native;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use kagari_bytecode::{ArtifactCompatibility, ArtifactValidationError, KbcArtifact};
use kagari_common::cancellation::CancellationToken;
use kagari_compiler::native_input::{NativeInputError, verify_native_input};
use kagari_mir::program::VerifiedMirProgram;
use kagari_runtime::{RuntimeError, VerifiedProgram};

use crate::program::native::{CachedFunction, NativeCacheKey};
pub use native::NativePreparationError;

/// Clones share bytecode, verified native input and compiled products. Loading a
/// clone creates fresh runtime instances and host bindings, never a new code cache.
/// Each program admits at most 4096 distinct cached function/configuration decisions.
#[derive(Debug, Clone)]
pub struct PreparedProgram {
    state: Rc<ProgramState>,
}

#[derive(Debug)]
struct ProgramState {
    bytecode: VerifiedProgram,
    mir: Option<VerifiedMirProgram>,
    native: RefCell<HashMap<NativeCacheKey, CachedFunction>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProgramPreparationError {
    #[error("program preparation cancelled")]
    Cancelled,
    #[error("invalid artifact: {0}")]
    Artifact(#[from] ArtifactValidationError),
    #[error("invalid native input: {0}")]
    NativeInput(NativeInputError),
    #[error("invalid executable program: {0}")]
    Runtime(#[from] RuntimeError),
}

impl From<NativeInputError> for ProgramPreparationError {
    fn from(error: NativeInputError) -> Self {
        match error {
            NativeInputError::Cancelled => Self::Cancelled,
            error => Self::NativeInput(error),
        }
    }
}

impl PreparedProgram {
    /// Validate all supplied compiler input before runtime linking or script effects.
    /// A missing payload is valid bytecode-only input; an invalid payload is an error.
    pub fn from_artifact(
        artifact: KbcArtifact,
        compatibility: &ArtifactCompatibility,
        cancel: &CancellationToken,
    ) -> Result<Self, ProgramPreparationError> {
        cancel
            .check()
            .map_err(|_| ProgramPreparationError::Cancelled)?;
        artifact.validate_for_loader(compatibility)?;
        let mir = artifact
            .portable_mir
            .as_ref()
            .map(|payload| verify_native_input(&payload.bytes, &artifact.program, cancel))
            .transpose()?;
        let bytecode = VerifiedProgram::new(artifact.program)?;
        cancel
            .check()
            .map_err(|_| ProgramPreparationError::Cancelled)?;
        Ok(Self {
            state: Rc::new(ProgramState {
                bytecode,
                mir,
                native: RefCell::default(),
            }),
        })
    }

    pub fn bytecode(&self) -> &VerifiedProgram {
        &self.state.bytecode
    }

    /// Verified compiler input exists; backend support remains function-specific.
    pub fn has_native_input(&self) -> bool {
        self.state.mir.is_some()
    }
}
