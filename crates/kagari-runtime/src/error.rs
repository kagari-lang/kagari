use crate::error_trace::ErrorTrace;
use std::sync::Arc;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    Cancelled,
    EngineFault,
    ScriptTrap,
    IndexOutOfBounds,
    ExecutionPhaseViolation,
    InvalidReflectiveRead,
    InvalidReflectiveWrite,
    ExpiredHostBorrow,
    HostBorrowConflict,
    HostBorrowEscape,
    HostCallFailure,
    TypedPathValidation,
    ModuleValidation,
    ResourceLimitExceeded,
    MetadataConflict,
    StaleHandle,
}

impl RuntimeErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "KG_RUNTIME_CANCELLED",
            Self::EngineFault => "KG_RUNTIME_ENGINE_FAULT",
            Self::ScriptTrap => "KG_RUNTIME_SCRIPT_TRAP",
            Self::IndexOutOfBounds => "KG_RUNTIME_INDEX_OUT_OF_BOUNDS",
            Self::ExecutionPhaseViolation => "KG_RUNTIME_EXECUTION_PHASE_VIOLATION",
            Self::InvalidReflectiveRead => "KG_RUNTIME_INVALID_REFLECTIVE_READ",
            Self::InvalidReflectiveWrite => "KG_RUNTIME_INVALID_REFLECTIVE_WRITE",
            Self::ExpiredHostBorrow => "KG_RUNTIME_EXPIRED_HOST_BORROW",
            Self::HostBorrowConflict => "KG_RUNTIME_HOST_BORROW_CONFLICT",
            Self::HostBorrowEscape => "KG_RUNTIME_HOST_BORROW_ESCAPE",
            Self::HostCallFailure => "KG_RUNTIME_HOST_CALL_FAILURE",
            Self::TypedPathValidation => "KG_RUNTIME_TYPED_PATH_VALIDATION",
            Self::ModuleValidation => "KG_RUNTIME_MODULE_VALIDATION",
            Self::ResourceLimitExceeded => "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED",
            Self::MetadataConflict => "KG_RUNTIME_METADATA_CONFLICT",
            Self::StaleHandle => "KG_RUNTIME_STALE_HANDLE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RuntimeError {
    kind: RuntimeErrorKind,
    message: String,
    trace: Option<Arc<ErrorTrace>>,
}

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            trace: None,
        }
    }

    pub fn execution_phase_violation(operation: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::ExecutionPhaseViolation,
            format!("execution phase forbids: {}", operation.into()),
        )
    }

    pub fn resource_limit(limit: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::ResourceLimitExceeded,
            format!("resource limit exceeded: {}", limit.into()),
        )
    }

    pub fn metadata_conflict(name: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::MetadataConflict,
            format!("metadata conflict: {}", name.into()),
        )
    }

    pub fn expired_host_borrow(detail: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::ExpiredHostBorrow,
            format!("expired host borrow: {}", detail.into()),
        )
    }

    pub fn host_borrow_conflict(detail: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::HostBorrowConflict,
            format!("host borrow conflict: {}", detail.into()),
        )
    }

    pub fn host_borrow_escape(detail: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::HostBorrowEscape,
            format!("host borrow escape: {}", detail.into()),
        )
    }

    pub fn invalid_reflective_read(detail: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::InvalidReflectiveRead,
            format!("invalid reflective read: {}", detail.into()),
        )
    }

    pub fn invalid_reflective_write(detail: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::InvalidReflectiveWrite,
            format!("invalid reflective write: {}", detail.into()),
        )
    }

    pub fn host_call_failure(detail: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::HostCallFailure,
            format!("host call failed: {}", detail.into()),
        )
    }

    pub fn typed_path_validation(detail: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::TypedPathValidation,
            format!("typed path validation failed: {}", detail.into()),
        )
    }

    pub fn module_validation(detail: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::ModuleValidation,
            format!("module validation failed: {}", detail.into()),
        )
    }

    pub fn trace(&self) -> Option<&Arc<ErrorTrace>> {
        self.trace.as_ref()
    }
    pub fn with_trace(mut self, trace: Arc<ErrorTrace>) -> Self {
        if self
            .trace
            .as_ref()
            .is_none_or(|previous| previous.frames.is_empty())
        {
            self.trace = Some(trace);
        }
        self
    }
    pub fn kind(&self) -> RuntimeErrorKind {
        self.kind
    }

    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeError, RuntimeErrorKind};

    #[test]
    fn runtime_errors_expose_stable_codes() {
        let error = RuntimeError::execution_phase_violation("external candidate access");

        assert_eq!(
            RuntimeErrorKind::ExecutionPhaseViolation.code(),
            "KG_RUNTIME_EXECUTION_PHASE_VIOLATION"
        );
        assert_eq!(error.code(), "KG_RUNTIME_EXECUTION_PHASE_VIOLATION");
        assert_eq!(error.kind(), RuntimeErrorKind::ExecutionPhaseViolation);
        assert_eq!(
            RuntimeErrorKind::IndexOutOfBounds.code(),
            "KG_RUNTIME_INDEX_OUT_OF_BOUNDS"
        );
    }
}
