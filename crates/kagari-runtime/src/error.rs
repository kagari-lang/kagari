use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    CapabilityDenied,
    InvalidReflectiveWrite,
    ExpiredHostBorrow,
    ResourceLimitExceeded,
    MetadataConflict,
    StaleHandle,
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

    pub fn kind(&self) -> RuntimeErrorKind {
        self.kind
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
