//! Shared immutable executable products, independent of Runtime installation state.
#[cfg(all(feature = "aot-build", not(target_family = "wasm")))]
pub mod aot;
#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "source")]
pub mod update;

#[cfg(feature = "source")]
use crate::engine::source::{
    PreparationOptions, compilation::Compilation, inputs::CompilationInputs,
};
#[cfg(feature = "source")]
use crate::{CompileResult, error::CompilationPhase};
use crate::{
    ReloadResult, RunResult,
    error::{EmbeddingError, RuntimeFailureKind},
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
    module::{VerifiedProgram, owner::ProgramCodeOwner},
    reload::{ReloadValidationError, prepare_program_update, updates::RetainedDemands},
};
use std::{
    any::Any,
    sync::{Arc, Weak},
};

/// Clones share immutable bytecode, checked source inputs and optional verified MIR.
/// Loading creates local state/authority. Native compilation caches belong to a
/// separate preparation owner; this product can be shared across worker threads.
#[derive(Debug, Clone)]
pub struct Program {
    state: Arc<ProgramState>,
}

#[derive(Debug)]
struct ProgramState {
    bytecode: VerifiedProgram,
    history: Arc<RetainedDemands>,
    expected_base: Option<Weak<ProgramState>>,
    #[cfg(feature = "source")]
    compilation: Option<Arc<CompilationInputs>>,
    #[cfg(feature = "native")]
    mir: Option<Arc<VerifiedMirProgram>>,
}

impl ProgramCodeOwner for ProgramState {}

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
    #[error("invalid update demand history: {0}")]
    History(ReloadValidationError),
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

impl Program {
    /// Keep this entire product alive through exact installed code. The product's
    /// own bytecode has no owner back-reference, so the two wrappers form no cycle.
    /// Transfer checked code to the core embedding API while retaining this entire
    /// preparation product. Bare `bytecode()` snapshots do not attach that owner.
    pub fn into_verified(self) -> VerifiedProgram {
        assert!(
            self.state.bytecode.owner().is_none(),
            "product code must not retain its installation wrapper"
        );
        self.state.bytecode.clone().with_owner(self.state)
    }

    pub(crate) fn from_installed_code(code: &VerifiedProgram) -> RunResult<Self> {
        let missing = || {
            EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                "MissingCompilationContext: current publication has no matching SDK Program owner",
            )
        };
        let owner: Arc<dyn Any + Send + Sync> = code.owner().ok_or_else(missing)?;
        let state = owner.downcast::<ProgramState>().map_err(|_| missing())?;
        if !state.bytecode.same_version(code) {
            return Err(missing());
        }
        Ok(Self { state })
    }

    /// Check an independently prepared candidate against this exact base without
    /// borrowing a Runtime. Code, source and MIR backings are reused; the result
    /// owns bounded cumulative history and a weak base identity, never base code.
    /// Already-bound candidates reject another base. Local replacement independently
    /// checks installation state, host bindings and historical executable coverage.
    pub fn prepare_replacement(
        &self,
        candidate: &Self,
        cancel: &CancellationToken,
    ) -> ReloadResult<Self> {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        if candidate.state.expected_base.is_some() {
            if candidate.expects_base(self) {
                return Ok(candidate.clone());
            }
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::StaleModuleOrHostRoot,
                "candidate was prepared for a different Program base",
            ));
        }
        let history = prepare_program_update(
            self.bytecode(),
            &self.state.history,
            candidate.bytecode(),
            cancel,
        )
        .map_err(|error| {
            if cancel.check().is_err() {
                EmbeddingError::Cancelled
            } else {
                EmbeddingError::reload_validation(error)
            }
        })?;
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        Ok(Self {
            state: Arc::new(ProgramState {
                bytecode: candidate.state.bytecode.clone(),
                history,
                expected_base: Some(Arc::downgrade(&self.state)),
                #[cfg(feature = "source")]
                compilation: candidate.state.compilation.clone(),
                #[cfg(feature = "native")]
                mir: candidate.state.mir.clone(),
            }),
        })
    }

    pub(crate) fn expects_base(&self, base: &Self) -> bool {
        self.state
            .expected_base
            .as_ref()
            .is_some_and(|expected| Weak::ptr_eq(expected, &Arc::downgrade(&base.state)))
    }

    /// Adopt products paired by the SDK's private source lowering owner.
    #[cfg(feature = "source")]
    pub(crate) fn from_compilation(
        compilation: Compilation,
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
        let history = match base {
            Some(base) => prepare_program_update(
                base.bytecode(),
                &base.state.history,
                &bytecode,
                &options.lowering.cancel,
            ),
            None => RetainedDemands::initial(&bytecode, &options.lowering.cancel),
        }
        .map_err(|error| {
            if options.lowering.cancel.check().is_err() {
                EmbeddingError::Cancelled
            } else {
                EmbeddingError::reload_validation(error)
            }
        })?;
        Ok(Self {
            state: Arc::new(ProgramState {
                bytecode,
                history,
                expected_base: base.map(|base| Arc::downgrade(&base.state)),
                compilation: Some(inputs),
                #[cfg(feature = "native")]
                mir: options.retain_native_input.then(|| Arc::new(mir)),
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
        let history = RetainedDemands::initial(&bytecode, cancel).map_err(|error| {
            if cancel.check().is_err() {
                ProgramPreparationError::Cancelled
            } else {
                ProgramPreparationError::History(error)
            }
        })?;
        Ok(Self {
            state: Arc::new(ProgramState {
                bytecode,
                history,
                expected_base: None,
                #[cfg(feature = "source")]
                compilation: None,
                #[cfg(feature = "native")]
                mir: mir.map(Arc::new),
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

#[cfg(test)]
mod tests;
