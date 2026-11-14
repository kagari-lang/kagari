//! SDK diagnostics and execution error mapping.
use kagari_bytecode::artifact::ArtifactValidationError;
#[cfg(feature = "source")]
use kagari_compiler::{bytecode::BytecodeLoweringError, source::lower::MirLoweringError};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    program::ProgramPreparationError,
    reload::ReloadValidationError as RuntimeReloadValidationError,
    task::TaskFailureOrigin,
};
#[cfg(feature = "source")]
use kagari_source::{diagnostic::Diagnostic, presentation::DiagnosticRecord, source::SourceFile};
#[cfg(feature = "source")]
use smallvec::SmallVec;
#[cfg(feature = "source")]
use std::{io, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationPhase {
    Parse,
    Analyze,
    MirLowering,
    BytecodeLowering,
    ArtifactEncoding,
}

impl CompilationPhase {
    pub fn code(self) -> &'static str {
        match self {
            Self::Parse => "KG_COMPILE_PARSE",
            Self::Analyze => "KG_COMPILE_ANALYZE",
            Self::MirLowering => "KG_COMPILE_IR_LOWERING",
            Self::BytecodeLowering => "KG_COMPILE_BYTECODE_LOWERING",
            Self::ArtifactEncoding => "KG_COMPILE_ARTIFACT_ENCODING",
        }
    }
}

#[derive(Debug)]
pub enum EmbeddingError {
    Registration {
        error: RuntimeError,
    },
    #[cfg(feature = "source")]
    DeclarationCache {
        path: PathBuf,
        error: io::Error,
    },
    Source {
        message: String,
    },
    Cancelled,
    #[cfg(feature = "source")]
    Diagnostics {
        diagnostics: Vec<DiagnosticRecord>,
    },
    Compilation {
        phase: CompilationPhase,
        message: String,
    },
    ArtifactValidation {
        error: ArtifactValidationError,
    },
    Runtime {
        error: RuntimeError,
    },
    ReloadValidation {
        code: String,
        message: String,
    },
}

impl EmbeddingError {
    pub fn task_origin(&self) -> Option<TaskFailureOrigin> {
        match self {
            Self::Runtime { error } => error.task_origin(),
            _ => None,
        }
    }

    pub fn error_trace(&self) -> Option<&ErrorTrace> {
        match self {
            Self::Runtime { error } => error.trace().map(AsRef::as_ref),
            _ => None,
        }
    }

    pub fn code(&self) -> String {
        match self {
            Self::Registration { error } => error.code().to_owned(),
            #[cfg(feature = "source")]
            Self::DeclarationCache { .. } => "KG_DECLARATION_CACHE".to_owned(),
            Self::Source { .. } => "KG_SOURCE_INPUT".to_owned(),
            Self::Cancelled => "KG_ANALYSIS_CANCELLED".to_owned(),
            #[cfg(feature = "source")]
            Self::Diagnostics { diagnostics } => diagnostics
                .first()
                .map(|diagnostic| diagnostic.code.clone())
                .unwrap_or_else(|| "KG_DIAGNOSTIC_EMPTY".to_owned()),
            Self::Compilation { phase, .. } => phase.code().to_owned(),
            Self::ArtifactValidation { error } => error.code().to_owned(),
            Self::Runtime { error } => error.code().to_owned(),
            Self::ReloadValidation { code, .. } => code.clone(),
        }
    }

    #[cfg(feature = "source")]
    pub(crate) fn diagnostics(
        diagnostics: Box<SmallVec<[Diagnostic; 4]>>,
        source: &SourceFile,
    ) -> Self {
        Self::Diagnostics {
            diagnostics: diagnostics
                .into_vec()
                .into_iter()
                .map(|diagnostic| DiagnosticRecord::from_diagnostic(diagnostic, source))
                .collect(),
        }
    }

    #[cfg(feature = "source")]
    pub(crate) fn ir_lowering(error: MirLoweringError, source: &SourceFile) -> Self {
        if let MirLoweringError::Cancelled = error {
            return Self::Cancelled;
        }
        if let MirLoweringError::Diagnostic(diagnostic) = error {
            return Self::diagnostics(Box::new(smallvec::smallvec![*diagnostic]), source);
        }
        Self::Compilation {
            phase: CompilationPhase::MirLowering,
            message: format!("{error:?}"),
        }
    }

    #[cfg(feature = "source")]
    pub(crate) fn bytecode_lowering(error: BytecodeLoweringError) -> Self {
        Self::Compilation {
            phase: CompilationPhase::BytecodeLowering,
            message: format!("{error:?}"),
        }
    }

    #[cfg(feature = "source")]
    pub(crate) fn artifact_validation(error: ArtifactValidationError) -> Self {
        Self::ArtifactValidation { error }
    }

    pub(crate) fn reload_validation(error: RuntimeReloadValidationError) -> Self {
        Self::ReloadValidation {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

impl From<RuntimeError> for EmbeddingError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime { error }
    }
}

impl From<ProgramPreparationError> for EmbeddingError {
    fn from(error: ProgramPreparationError) -> Self {
        match error {
            ProgramPreparationError::Cancelled => Self::Cancelled,
            ProgramPreparationError::Artifact(error) => Self::ArtifactValidation { error },
            ProgramPreparationError::SourcePackage(error) => error.into(),
            ProgramPreparationError::History(error) => Self::reload_validation(error),
            ProgramPreparationError::Identity(error) => Self::Source {
                message: error.to_string(),
            },
        }
    }
}

impl From<RuntimeReloadValidationError> for EmbeddingError {
    fn from(error: RuntimeReloadValidationError) -> Self {
        match error {
            RuntimeReloadValidationError::Runtime(error)
                if error.kind() == RuntimeErrorKind::Cancelled =>
            {
                Self::Cancelled
            }
            error => Self::reload_validation(error),
        }
    }
}
