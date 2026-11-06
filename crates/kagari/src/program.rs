//! Preparation-side executable input and local compilation cache, independent of runtimes.
#[cfg(all(feature = "aot-build", not(target_family = "wasm")))]
pub mod aot;
#[cfg(any(feature = "source", feature = "native"))]
pub(crate) mod inputs;
#[cfg(feature = "native")]
pub mod native;

#[cfg(any(feature = "source", feature = "native"))]
use crate::program::inputs::RetainedPreparation;
#[cfg(feature = "native")]
use std::cell::RefCell;
#[cfg(feature = "native")]
use std::collections::HashMap;
use std::rc::Rc;
#[cfg(feature = "source")]
use std::sync::Arc;

#[cfg(feature = "source")]
use crate::engine::source::{
    PreparationOptions, compilation::Compilation, inputs::CompilationInputs,
};
#[cfg(feature = "source")]
use crate::{
    CompileResult,
    error::{CompilationPhase, EmbeddingError},
};
use kagari_bytecode::{
    artifact::{ArtifactCompatibility, ArtifactValidationError, KbcArtifact},
    verifier::BytecodeVerificationError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext, mapping::DefinitionMappingError, metadata::DefinitionMetadata,
    },
};
#[cfg(feature = "native")]
use kagari_compiler::native_input::{NativeInputError, verify_native_input};
#[cfg(feature = "native")]
use kagari_mir::program::VerifiedMirProgram;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    module::VerifiedProgram,
};

#[cfg(feature = "native")]
use crate::program::native::{CachedFunction, NativeCacheKey};

/// Clones share bytecode, verified native input and compiled products. Loading a
/// clone creates fresh runtime instances and host bindings, never a new code cache.
/// Each program admits at most 4096 distinct cached function/configuration decisions.
/// This preparation-side Rc/RefCell owner is neither Send nor Sync. A loaded runtime
/// does not retain it and can move independently, including its installed native code.
#[derive(Debug, Clone)]
pub struct PreparedProgram {
    state: Rc<ProgramState>,
}

#[derive(Debug)]
struct ProgramState {
    bytecode: VerifiedProgram,
    #[cfg(feature = "source")]
    compilation: Option<Arc<CompilationInputs>>,
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
    #[error("invalid definition scope: {0}")]
    Identity(DefinitionMappingError),
    #[error("invalid artifact: {0}")]
    Artifact(ArtifactValidationError),
    #[error("invalid source package attachment: {0}")]
    SourcePackage(RuntimeError),
    #[cfg(feature = "native")]
    #[error("invalid native input: {0}")]
    NativeInput(NativeInputError),
}

impl From<ArtifactValidationError> for ProgramPreparationError {
    fn from(error: ArtifactValidationError) -> Self {
        match error {
            ArtifactValidationError::Identity(DefinitionMappingError::Cancelled)
            | ArtifactValidationError::Bytecode(BytecodeVerificationError::Cancelled) => {
                Self::Cancelled
            }
            error => Self::Artifact(error),
        }
    }
}

impl From<DefinitionMappingError> for ProgramPreparationError {
    fn from(error: DefinitionMappingError) -> Self {
        match error {
            DefinitionMappingError::Cancelled => Self::Cancelled,
            error => Self::Identity(error),
        }
    }
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
    /// Adopt products paired by the SDK's private source lowering owner.
    #[cfg(feature = "source")]
    pub(crate) fn from_compilation(
        compilation: Compilation,
        options: &PreparationOptions,
    ) -> CompileResult<Self> {
        let inputs = Arc::new(compilation.inputs().with_preparation(options));
        let (bytecode, mir, source) = compilation.into_parts();
        let bytecode = VerifiedProgram::from_bytecode(bytecode)
            .with_source_package(source, &options.lowering.cancel)
            .map_err(|error| {
                if error.kind() == RuntimeErrorKind::Cancelled {
                    EmbeddingError::Cancelled
                } else {
                    EmbeddingError::Compilation {
                        phase: CompilationPhase::BytecodeLowering,
                        message: format!("source attachment: {error}"),
                    }
                }
            })?;
        #[cfg(not(feature = "native"))]
        let _ = (mir, options);
        Ok(Self {
            state: Rc::new(ProgramState {
                bytecode,
                compilation: Some(inputs),
                #[cfg(feature = "native")]
                mir: options.retain_native_input.then_some(mir),
                #[cfg(feature = "native")]
                native: RefCell::default(),
            }),
        })
    }

    /// Validate the artifact before runtime linking or script effects. Native-enabled
    /// builds additionally validate all supplied compiler input; bytecode-only builds
    /// check the opaque payload bounds and integrity without interpreting MIR.
    /// Scoped input is resource-checked before adoption into the preparation context;
    /// importing identities does not replace artifact or executable verification.
    /// A missing payload is valid bytecode-only input. Native-enabled builds reject
    /// malformed or mismatched compiler input.
    pub fn from_artifact(
        artifact: DefinitionMetadata<KbcArtifact>,
        compatibility: &ArtifactCompatibility,
        cancel: &CancellationToken,
    ) -> Result<Self, ProgramPreparationError> {
        cancel
            .check()
            .map_err(|_| ProgramPreparationError::Cancelled)?;
        artifact
            .records()
            .validate_resource_limits(artifact.definitions(), cancel)?;
        let context = DefinitionContext::new().map_err(DefinitionMappingError::from)?;
        let artifact = artifact.import_into(&context, cancel)?.into_records();
        let artifact = artifact.into_verified(compatibility, &context, cancel)?;
        #[cfg(feature = "native")]
        let mir = artifact
            .portable_mir()
            .map(|payload| verify_native_input(&payload.bytes, artifact.bytecode(), cancel))
            .transpose()?;
        let (bytecode, _, source) = artifact.into_parts();
        let mut bytecode = VerifiedProgram::from_bytecode(bytecode);
        if let Some(source) = source {
            bytecode = bytecode
                .with_source_package(source, cancel)
                .map_err(|error| {
                    if error.kind() == RuntimeErrorKind::Cancelled {
                        ProgramPreparationError::Cancelled
                    } else {
                        ProgramPreparationError::SourcePackage(error)
                    }
                })?;
        }
        cancel
            .check()
            .map_err(|_| ProgramPreparationError::Cancelled)?;
        Ok(Self {
            state: Rc::new(ProgramState {
                bytecode,
                #[cfg(feature = "source")]
                compilation: None,
                #[cfg(feature = "native")]
                mir,
                #[cfg(feature = "native")]
                native: RefCell::default(),
            }),
        })
    }

    /// Detach immutable compiler inputs without retaining this preparation cache.
    #[cfg(any(feature = "source", feature = "native"))]
    pub(crate) fn retained_inputs(&self) -> RetainedPreparation {
        RetainedPreparation {
            #[cfg(feature = "source")]
            compilation: self.state.compilation.clone(),
            #[cfg(feature = "native")]
            mir: self.state.mir.clone(),
        }
    }

    #[cfg(feature = "native")]
    pub(crate) fn from_current(bytecode: VerifiedProgram, inputs: RetainedPreparation) -> Self {
        Self {
            state: Rc::new(ProgramState {
                bytecode,
                #[cfg(feature = "source")]
                compilation: inputs.compilation,
                mir: inputs.mir,
                native: RefCell::default(),
            }),
        }
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

#[cfg(test)]
mod tests;
