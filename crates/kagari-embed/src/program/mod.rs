//! Shared immutable executable input, independent of runtime instances.
#[cfg(feature = "native")]
pub mod native;

#[cfg(feature = "native")]
use std::cell::RefCell;
#[cfg(feature = "native")]
use std::collections::HashMap;
use std::rc::Rc;

use kagari_bytecode::artifact::{ArtifactCompatibility, ArtifactValidationError, KbcArtifact};
use kagari_common::cancellation::CancellationToken;
#[cfg(feature = "native")]
use kagari_compiler::native_input::{NativeInputError, verify_native_input};
#[cfg(feature = "native")]
use kagari_mir::program::VerifiedMirProgram;
use kagari_runtime::{error::RuntimeError, module::VerifiedProgram};

#[cfg(feature = "native")]
use crate::program::native::{CachedFunction, NativeCacheKey};

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
    #[cfg(feature = "native")]
    mir: Option<VerifiedMirProgram>,
    #[cfg(feature = "native")]
    native: RefCell<HashMap<NativeCacheKey, CachedFunction>>,
}

/// Preparation failures include feature-dependent native validation. Consumers
/// must handle unknown variants because Cargo can unify features through another
/// dependency even when the consumer itself does not request native compilation.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProgramPreparationError {
    #[error("program preparation cancelled")]
    Cancelled,
    #[error("invalid artifact: {0}")]
    Artifact(#[from] ArtifactValidationError),
    #[cfg(feature = "native")]
    #[error("invalid native input: {0}")]
    NativeInput(NativeInputError),
    #[error("invalid executable program: {0}")]
    Runtime(#[from] RuntimeError),
}

#[cfg(feature = "native")]
impl From<NativeInputError> for ProgramPreparationError {
    fn from(error: NativeInputError) -> Self {
        match error {
            NativeInputError::Cancelled => Self::Cancelled,
            error => Self::NativeInput(error),
        }
    }
}

impl PreparedProgram {
    /// Validate the artifact before runtime linking or script effects. Native-enabled
    /// builds additionally validate all supplied compiler input; bytecode-only builds
    /// check the opaque payload bounds and integrity without interpreting MIR.
    /// A missing payload is valid bytecode-only input. Native-enabled builds reject
    /// malformed or mismatched compiler input.
    pub fn from_artifact(
        artifact: KbcArtifact,
        compatibility: &ArtifactCompatibility,
        cancel: &CancellationToken,
    ) -> Result<Self, ProgramPreparationError> {
        cancel
            .check()
            .map_err(|_| ProgramPreparationError::Cancelled)?;
        artifact.validate_for_loader(compatibility)?;
        #[cfg(feature = "native")]
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
                #[cfg(feature = "native")]
                mir,
                #[cfg(feature = "native")]
                native: RefCell::default(),
            }),
        })
    }

    pub fn bytecode(&self) -> &VerifiedProgram {
        &self.state.bytecode
    }

    /// Verified compiler input exists; backend support remains function-specific.
    #[cfg(feature = "native")]
    pub fn has_native_input(&self) -> bool {
        self.state.mir.is_some()
    }
}
