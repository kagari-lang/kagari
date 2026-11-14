//! Shared immutable executable products, independent of preparation and installation.
use crate::{
    error::{
        RuntimeError, RuntimeErrorKind,
        reason::{CancellationReason, StaleHandleReason},
    },
    module::VerifiedProgram,
    reload::{ReloadValidationError, prepare_program_update, updates::RetainedDemands},
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
use std::sync::{Arc, Weak};

/// Immutable executable code, checked layouts and exact replacement history.
/// Clones share the product; installations, statics and heap state stay Runtime-local.
/// Source/MIR preparation inputs are not retained by this executable owner.
#[derive(Debug, Clone)]
pub struct Program {
    state: Arc<ProgramState>,
}

#[derive(Debug)]
struct ProgramState {
    bytecode: VerifiedProgram,
    history: Arc<RetainedDemands>,
    expected_base: Option<Weak<ProgramState>>,
}

/// Preparation failures for source-independent executable products.
#[derive(Debug, thiserror::Error)]
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

impl Program {
    /// Adopt verified backing without rechecking code bodies or acquiring Runtime authority.
    /// Preserves an existing typed product association, including replacement history.
    pub fn from_verified(
        bytecode: VerifiedProgram,
        cancel: &CancellationToken,
    ) -> Result<Self, ProgramPreparationError> {
        cancel
            .check()
            .map_err(|_| ProgramPreparationError::Cancelled)?;
        if let Some(program) = bytecode.product() {
            return Ok(program.clone());
        }
        let history = RetainedDemands::initial(&bytecode, cancel).map_err(|error| {
            if cancel.check().is_err() {
                ProgramPreparationError::Cancelled
            } else {
                ProgramPreparationError::History(error)
            }
        })?;
        Ok(Self::with_checked_history(bytecode, history))
    }

    /// Adopt history already checked by detached preparation or local admission.
    /// The backing must not own a product, preventing a reverse owning cycle.
    pub(crate) fn with_checked_history(
        bytecode: VerifiedProgram,
        history: Arc<RetainedDemands>,
    ) -> Self {
        assert!(
            bytecode.product().is_none(),
            "product backing cannot own itself"
        );
        Self {
            state: Arc::new(ProgramState {
                bytecode,
                history,
                expected_base: None,
            }),
        }
    }

    /// Transfer verified code with its typed executable lifetime owner.
    /// The product's own backing has no reverse association, so no owning cycle forms.
    pub fn into_verified(self) -> VerifiedProgram {
        assert!(
            self.state.bytecode.product().is_none(),
            "product backing cannot own itself"
        );
        self.state.bytecode.clone().with_product(self)
    }

    /// Check one detached replacement against this exact base and cumulative history.
    /// Cancellation or failure leaves both immutable products unchanged.
    pub fn prepare_replacement(
        &self,
        candidate: &Self,
        cancel: &CancellationToken,
    ) -> Result<Self, ReloadValidationError> {
        cancel.check().map_err(|_| {
            ReloadValidationError::Runtime(RuntimeError::from(
                CancellationReason::ExecutionCancelled,
            ))
        })?;
        if candidate.state.expected_base.is_some() {
            if candidate.expects_base(self) {
                return Ok(candidate.clone());
            }
            return Err(ReloadValidationError::Runtime(RuntimeError::from(
                StaleHandleReason::InstallationPublicationBaselineChangedOrClosed,
            )));
        }
        let history = prepare_program_update(
            self.bytecode(),
            &self.state.history,
            candidate.bytecode(),
            cancel,
        )?;
        cancel.check().map_err(|_| {
            ReloadValidationError::Runtime(RuntimeError::from(
                CancellationReason::ExecutionCancelled,
            ))
        })?;
        Ok(Self {
            state: Arc::new(ProgramState {
                bytecode: candidate.state.bytecode.clone(),
                history,
                expected_base: Some(Arc::downgrade(&self.state)),
            }),
        })
    }

    /// Whether this candidate was prepared against the supplied exact product base.
    /// This carries no installation/publication authority.
    pub fn expects_base(&self, base: &Self) -> bool {
        self.state
            .expected_base
            .as_ref()
            .is_some_and(|expected| Weak::ptr_eq(expected, &Arc::downgrade(&base.state)))
    }

    /// Verified code/layout backing, without the installed product back-reference.
    pub fn bytecode(&self) -> &VerifiedProgram {
        &self.state.bytecode
    }

    /// Bounded cumulative executable demands used by detached compiler preparation.
    pub fn retained_demands(&self) -> &RetainedDemands {
        &self.state.history
    }

    /// Validate portable executable input without source analysis or MIR compilation.
    /// Opaque compiler payload bounds/integrity are checked by artifact verification;
    /// using that payload for backend compilation requires separate SDK preparation.
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
        Self::from_verified(bytecode, cancel)
    }
}
