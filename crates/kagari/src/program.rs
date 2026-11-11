//! Executable facade and explicit compiler preparation ownership.
#[cfg(all(feature = "aot-build", not(target_family = "wasm")))]
pub mod aot;
#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "source")]
pub mod update;

pub use kagari_runtime::program::{Program, ProgramPreparationError};

#[cfg(feature = "source")]
use crate::error::EmbeddingError;
#[cfg(feature = "source")]
use crate::{
    CompileResult,
    engine::source::{
        PreparationOptions, compilation::Compilation as LoweredCompilation,
        inputs::CompilationInputs,
    },
    error::CompilationPhase,
};
use kagari_bytecode::artifact::{ArtifactCompatibility, ArtifactValidationError, KbcArtifact};
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
use kagari_runtime::{error::RuntimeErrorKind, module::VerifiedProgram};
#[cfg(any(feature = "source", feature = "native"))]
use std::sync::Arc;

/// Explicit source/MIR preparation for one canonical executable Program.
/// Runtime accepts `program()` and never retains this preparation owner.
/// Keep this value to prepare source edits or native code; installed code cannot recover it.
#[derive(Debug, Clone)]
pub struct Compilation {
    program: Program,
    #[cfg(feature = "source")]
    compilation: Option<Arc<CompilationInputs>>,
    #[cfg(feature = "native")]
    mir: Option<Arc<VerifiedMirProgram>>,
}

/// Executable validation and optional backend-input validation have distinct owners.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompilationPreparationError {
    #[error("{0}")]
    Program(#[from] ProgramPreparationError),
    #[cfg(feature = "native")]
    #[error("invalid native input: {0}")]
    NativeInput(NativeInputError),
}

impl From<ArtifactValidationError> for CompilationPreparationError {
    fn from(error: ArtifactValidationError) -> Self {
        Self::Program(error.into())
    }
}

impl From<DefinitionMappingError> for CompilationPreparationError {
    fn from(error: DefinitionMappingError) -> Self {
        Self::Program(error.into())
    }
}

#[cfg(feature = "native")]
impl From<NativeInputError> for CompilationPreparationError {
    fn from(error: NativeInputError) -> Self {
        if matches!(error, NativeInputError::Cancelled) {
            Self::Program(ProgramPreparationError::Cancelled)
        } else {
            Self::NativeInput(error)
        }
    }
}

impl Compilation {
    /// The exact canonical executable, independently shareable and installable.
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Discard preparation inputs and transfer the executable product.
    pub fn into_program(self) -> Program {
        self.program
    }

    /// Pair checked source/MIR with its executable revision in the preparation owner.
    #[cfg(feature = "source")]
    pub(crate) fn from_compilation(
        compilation: LoweredCompilation,
        options: &PreparationOptions,
        base: Option<&Self>,
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
        let program = Program::from_verified(bytecode, &options.lowering.cancel)
            .map_err(EmbeddingError::from)?;
        let program = match base {
            Some(base) => base
                .program
                .prepare_replacement(&program, &options.lowering.cancel)
                .map_err(EmbeddingError::from)?,
            None => program,
        };
        Ok(Self {
            program,
            compilation: Some(inputs),
            #[cfg(feature = "native")]
            mir: options.retain_native_input.then(|| Arc::new(mir)),
        })
    }

    /// Validate an artifact and its supplied backend input without source analysis.
    /// Missing source inputs do not gain update authority from diagnostic attachments.
    pub fn from_artifact(
        artifact: DefinitionMetadata<KbcArtifact>,
        compatibility: &ArtifactCompatibility,
        cancel: &CancellationToken,
    ) -> Result<Self, CompilationPreparationError> {
        cancel.check().map_err(|_| {
            CompilationPreparationError::Program(ProgramPreparationError::Cancelled)
        })?;
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
                        CompilationPreparationError::Program(ProgramPreparationError::Cancelled)
                    } else {
                        CompilationPreparationError::Program(
                            ProgramPreparationError::SourcePackage(error),
                        )
                    }
                })?;
        }
        cancel.check().map_err(|_| {
            CompilationPreparationError::Program(ProgramPreparationError::Cancelled)
        })?;
        let program = Program::from_verified(bytecode, cancel)?;
        Ok(Self {
            program,
            #[cfg(feature = "source")]
            compilation: None,
            #[cfg(feature = "native")]
            mir: mir.map(Arc::new),
        })
    }

    /// Verified compiler input exists; backend support remains function-specific.
    #[cfg(feature = "native")]
    pub fn has_native_input(&self) -> bool {
        self.mir.is_some()
    }
}

#[cfg(test)]
mod tests;
