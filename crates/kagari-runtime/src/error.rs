use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    CapabilityDenied,
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
            Self::CapabilityDenied => "KG_RUNTIME_CAPABILITY_DENIED",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    kind: RuntimeErrorKind,
    message: String,
}

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn capability_denied(capability: impl Into<String>) -> Self {
        Self::new(
            RuntimeErrorKind::CapabilityDenied,
            format!("capability denied: {}", capability.into()),
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

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

#[cfg(test)]
mod tests {
    use super::{RuntimeError, RuntimeErrorKind};

    #[test]
    fn runtime_errors_expose_stable_codes() {
        let error = RuntimeError::capability_denied("host_calls");

        assert_eq!(
            RuntimeErrorKind::CapabilityDenied.code(),
            "KG_RUNTIME_CAPABILITY_DENIED"
        );
        assert_eq!(error.code(), "KG_RUNTIME_CAPABILITY_DENIED");
        assert_eq!(error.kind(), RuntimeErrorKind::CapabilityDenied);
    }
}
