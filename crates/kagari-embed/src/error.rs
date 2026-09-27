//! SDK diagnostics and execution error mapping.
use kagari_bytecode::ArtifactValidationError;
#[cfg(feature = "source")]
use kagari_common::{Diagnostic, SourceFile};
use kagari_common::{Severity, identity::FileSpan};
#[cfg(feature = "source")]
use kagari_compiler::{MirLoweringError, bytecode::BytecodeLoweringError};
use kagari_runtime::{
    ErrorTrace, ReloadValidationError as RuntimeReloadValidationError, RuntimeError,
    RuntimeErrorKind,
};
use kagari_vm::VmError;
#[cfg(feature = "source")]
use smallvec::SmallVec;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingDiagnostic {
    pub severity: Severity,
    pub code: String,
    pub span: Option<FileSpan>,
    pub message: String,
    pub notes: Vec<String>,
    pub labels: Vec<DiagnosticLabel>,
}

impl EmbeddingDiagnostic {
    #[cfg(feature = "source")]
    pub(crate) fn from_diagnostic(diagnostic: Diagnostic, source: &SourceFile) -> Self {
        let span = diagnostic
            .span
            .and_then(|span| source.span(span))
            .map(|mut span| {
                span.file = source.origin_id();
                span
            });
        Self {
            severity: diagnostic.severity,
            code: diagnostic.kind.code().to_owned(),
            span,
            message: diagnostic.kind.to_string(),
            notes: Vec::new(),
            labels: span
                .into_iter()
                .map(|span| DiagnosticLabel {
                    span,
                    message: "primary".to_owned(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticLabel {
    pub span: FileSpan,
    pub message: String,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFailureKind {
    Cancelled,
    ScriptTrap,
    BytecodeVerification,
    CapabilityDenied,
    ResourceLimitExceeded,
    HostCallFailure,
    TypedPathValidation,
    StaleModuleOrHostRoot,
    ReloadValidation,
    EngineInvariant,
    UnsupportedExecution,
}

impl RuntimeFailureKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "KG_RUNTIME_CANCELLED",
            Self::ScriptTrap => "KG_RUNTIME_SCRIPT_TRAP",
            Self::BytecodeVerification => "KG_BYTECODE_VERIFICATION_FAILED",
            Self::CapabilityDenied => "KG_RUNTIME_CAPABILITY_DENIED",
            Self::ResourceLimitExceeded => "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED",
            Self::HostCallFailure => "KG_RUNTIME_HOST_CALL_FAILURE",
            Self::TypedPathValidation => "KG_RUNTIME_TYPED_PATH_VALIDATION",
            Self::StaleModuleOrHostRoot => "KG_RUNTIME_STALE_HANDLE",
            Self::ReloadValidation => "KG_RELOAD_VALIDATION_FAILED",
            Self::EngineInvariant => "KG_ENGINE_INVARIANT",
            Self::UnsupportedExecution => "KG_RUNTIME_UNSUPPORTED_EXECUTION",
        }
    }
}

#[derive(Debug)]
pub enum EmbeddingError {
    Source {
        message: String,
    },
    Cancelled,
    Diagnostics {
        diagnostics: Vec<EmbeddingDiagnostic>,
    },
    Compilation {
        phase: CompilationPhase,
        message: String,
    },
    ArtifactValidation {
        error: ArtifactValidationError,
    },
    Load {
        error: RuntimeError,
    },
    Runtime {
        kind: RuntimeFailureKind,
        message: String,
        trace: Option<Arc<ErrorTrace>>,
    },
    ReloadValidation {
        code: String,
        message: String,
    },
}

impl EmbeddingError {
    pub fn error_trace(&self) -> Option<&ErrorTrace> {
        match self {
            Self::Runtime { trace, .. } => trace.as_deref(),
            Self::Load { error } => error.trace().map(AsRef::as_ref),
            _ => None,
        }
    }

    pub fn code(&self) -> String {
        match self {
            Self::Source { .. } => "KG_SOURCE_INPUT".to_owned(),
            Self::Cancelled => "KG_ANALYSIS_CANCELLED".to_owned(),
            Self::Diagnostics { diagnostics } => diagnostics
                .first()
                .map(|diagnostic| diagnostic.code.clone())
                .unwrap_or_else(|| "KG_DIAGNOSTIC_EMPTY".to_owned()),
            Self::Compilation { phase, .. } => phase.code().to_owned(),
            Self::ArtifactValidation { error } => error.code().to_owned(),
            Self::Load { error } => error.code().to_owned(),
            Self::Runtime { kind, .. } => kind.code().to_owned(),
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
                .map(|diagnostic| EmbeddingDiagnostic::from_diagnostic(diagnostic, source))
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

    pub(crate) fn load(error: RuntimeError) -> Self {
        Self::Load { error }
    }

    pub(crate) fn runtime(kind: RuntimeFailureKind, message: impl Into<String>) -> Self {
        Self::Runtime {
            kind,
            message: message.into(),
            trace: None,
        }
    }

    pub(crate) fn reload_validation(error: impl Into<ReloadValidationError>) -> Self {
        let error = error.into();
        Self::ReloadValidation {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }

    pub(crate) fn vm(error: VmError) -> Self {
        let trace = error.trace().cloned();
        let kind = match error.cause() {
            VmError::Traced { .. } => unreachable!("unwrapped error"),
            VmError::HostError(_) => RuntimeFailureKind::HostCallFailure,
            VmError::RuntimeError(error) => match error.kind() {
                RuntimeErrorKind::Cancelled => RuntimeFailureKind::Cancelled,
                RuntimeErrorKind::EngineFault => RuntimeFailureKind::EngineInvariant,
                RuntimeErrorKind::CapabilityDenied => RuntimeFailureKind::CapabilityDenied,
                RuntimeErrorKind::ResourceLimitExceeded => {
                    RuntimeFailureKind::ResourceLimitExceeded
                }
                RuntimeErrorKind::StaleHandle => RuntimeFailureKind::StaleModuleOrHostRoot,
                RuntimeErrorKind::HostBorrowConflict
                | RuntimeErrorKind::HostBorrowEscape
                | RuntimeErrorKind::ExpiredHostBorrow
                | RuntimeErrorKind::TypedPathValidation => RuntimeFailureKind::TypedPathValidation,
                RuntimeErrorKind::HostCallFailure => RuntimeFailureKind::HostCallFailure,
                RuntimeErrorKind::ModuleValidation => RuntimeFailureKind::BytecodeVerification,
                RuntimeErrorKind::InvalidReflectiveRead
                | RuntimeErrorKind::ScriptTrap
                | RuntimeErrorKind::IndexOutOfBounds
                | RuntimeErrorKind::InvalidReflectiveWrite
                | RuntimeErrorKind::MetadataConflict => RuntimeFailureKind::ScriptTrap,
            },
            VmError::BytecodeVerification(_) => RuntimeFailureKind::BytecodeVerification,
            VmError::AmbiguousFunction(_)
            | VmError::InvalidFunctionRef(_)
            | VmError::InvalidModuleSlot(_)
            | VmError::UnsupportedCallTarget(_)
            | VmError::UnsupportedInstruction(_) => RuntimeFailureKind::BytecodeVerification,
            VmError::JitInvocation(_) => RuntimeFailureKind::EngineInvariant,
            VmError::MissingFunction(_)
            | VmError::MissingField(_)
            | VmError::ImmutableModuleSlot(_)
            | VmError::InvalidIndex(_)
            | VmError::InvalidBranchCondition
            | VmError::BuiltinError(_)
            | VmError::ReflectionError(_)
            | VmError::Trap(_)
            | VmError::TypeMismatch(_) => RuntimeFailureKind::ScriptTrap,
        };
        Self::Runtime {
            kind,
            message: match error.cause() {
                VmError::RuntimeError(e) => e.message().to_owned(),
                other => format!("{other:?}"),
            },
            trace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReloadValidationError {
    #[error("artifact validation failed: {0}")]
    Artifact(ArtifactValidationError),
    #[error("{0}")]
    Runtime(RuntimeReloadValidationError),
}

impl ReloadValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Artifact(error) => error.code(),
            Self::Runtime(error) => error.code(),
        }
    }
}

impl From<ArtifactValidationError> for ReloadValidationError {
    fn from(error: ArtifactValidationError) -> Self {
        Self::Artifact(error)
    }
}

impl From<RuntimeReloadValidationError> for ReloadValidationError {
    fn from(error: RuntimeReloadValidationError) -> Self {
        Self::Runtime(error)
    }
}
