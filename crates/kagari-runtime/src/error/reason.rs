//! Typed failure reasons; diagnostic context is owned by RuntimeError.
use crate::{
    error::{RuntimeError, RuntimeErrorKind, fault::EngineFault, trap::TrapReason},
    host::HostError,
};
use kagari_types::scalar::error::ScalarError;
use std::mem;

/// CancellationReason classification without a preformatted message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CancellationReason {
    #[error("branch activation abandoned")]
    BranchActivationAbandoned,

    #[error("declaration correspondence cancelled")]
    DeclarationCorrespondenceCancelled,

    #[error("execution cancelled")]
    ExecutionCancelled,

    #[error("execution receiver dropped")]
    ExecutionReceiverDropped,

    #[error("group parent abandoned")]
    GroupParentAbandoned,

    #[error("initial demand preparation cancelled")]
    InitialDemandPreparationCancelled,

    #[error("program identity admission cancelled")]
    ProgramIdentityAdmissionCancelled,

    #[error("program identity import cancelled")]
    ProgramIdentityImportCancelled,

    #[error("runtime is closed")]
    RuntimeIsClosed,

    #[error("scheduled execution cancelled")]
    ScheduledExecutionCancelled,

    #[error("source package attachment cancelled")]
    SourcePackageAttachmentCancelled,

    #[error("task cancelled")]
    TaskCancelled,

    #[error("task cancelled before start")]
    TaskCancelledBeforeStart,

    /// A cancellation diagnostic supplied by a Host implementation.
    #[error("{0}")]
    HostRequest(&'static str),
}

/// StaleHandleReason classification without a preformatted message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StaleHandleReason {
    #[error("executable module belongs to another runtime")]
    ExecutableModuleBelongsToAnotherRuntime,

    #[error("executable module has been released")]
    ExecutableModuleHasBeenReleased,

    #[error("foreign or unregistered installation")]
    ForeignOrUnregisteredInstallation,

    #[error("installation is closed")]
    InstallationIsClosed,

    #[error("installation publication baseline changed or closed")]
    InstallationPublicationBaselineChangedOrClosed,

    #[error("unregistered installation")]
    UnregisteredInstallation,
}

/// A Runtime index failure or a diagnostic owned by a Host/library provider.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    #[error("array copy range is out of bounds")]
    ArrayCopyRangeIsOutOfBounds,

    #[error("list index is out of bounds")]
    ListIndexIsOutOfBounds,

    #[error("sequence index is out of bounds")]
    SequenceIndexIsOutOfBounds,

    #[error("sequence set index is out of bounds")]
    SequenceSetIndexIsOutOfBounds,

    #[error("invalid index: {index}")]
    InvalidIndex { index: u64 },

    /// An index diagnostic supplied by a Host or library provider.
    #[error("{0}")]
    Provider(String),
}

/// A reflective operation preserves its underlying failure without flattening it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReflectiveFailure {
    #[error("{0}")]
    Detail(String),
    #[error(transparent)]
    Cause(Box<RuntimeError>),
}

impl ReflectiveFailure {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Detail(text) => text.capacity(),
            Self::Cause(reason) => mem::size_of::<RuntimeError>() + reason.retained_bytes(),
        }
    }
}

/// Authoritative failure identity. Public category/code is derived from this value.
/// Text details belong to cold validation/provider diagnostics, not numeric transport.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErrorReason {
    #[error(transparent)]
    Scalar(#[from] ScalarError),
    #[error(transparent)]
    Trap(#[from] TrapReason),
    #[error(transparent)]
    Cancelled(#[from] CancellationReason),
    #[error(transparent)]
    EngineFault(#[from] EngineFault),
    #[error(transparent)]
    StaleHandle(#[from] StaleHandleReason),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error("execution phase forbids: {0}")]
    ExecutionPhaseViolation(String),
    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),
    #[error("metadata conflict: {0}")]
    MetadataConflict(String),
    #[error("expired host borrow: {0}")]
    ExpiredHostBorrow(String),
    #[error("host borrow conflict: {0}")]
    HostBorrowConflict(String),
    #[error("host borrow escape: {0}")]
    HostBorrowEscape(String),
    #[error("invalid reflective read: {0}")]
    InvalidReflectiveRead(#[source] ReflectiveFailure),
    #[error("invalid reflective write: {0}")]
    InvalidReflectiveWrite(#[source] ReflectiveFailure),
    #[error("host call failed: {0}")]
    HostCallFailure(#[source] HostError),
    #[error("typed path validation failed: {0}")]
    TypedPathValidation(String),
    #[error("module validation failed: {0}")]
    ModuleValidation(String),
    #[error("native scalar conversion differs from its declaration")]
    NativeScalarMismatch,
    #[error("debug observation failed: {0}")]
    DebugObservationFailure(String),
    #[error("owned backend failure: {0}")]
    OwnedBackendFailure(String),
    #[error("invalid verified public target: {path:?} (expected {expected})")]
    InvalidVerifiedPublicTarget {
        path: String,
        expected: &'static str,
    },
}

impl ErrorReason {
    /// Stable outward category; callers cannot independently assign it.
    pub fn kind(&self) -> RuntimeErrorKind {
        match self {
            Self::Scalar(_) | Self::Trap(_) => RuntimeErrorKind::ScriptTrap,
            Self::Cancelled(_) => RuntimeErrorKind::Cancelled,
            Self::EngineFault(_)
            | Self::DebugObservationFailure(_)
            | Self::OwnedBackendFailure(_)
            | Self::InvalidVerifiedPublicTarget { .. } => RuntimeErrorKind::EngineFault,
            Self::StaleHandle(_) => RuntimeErrorKind::StaleHandle,
            Self::Index(_) => RuntimeErrorKind::IndexOutOfBounds,
            Self::ExecutionPhaseViolation(_) => RuntimeErrorKind::ExecutionPhaseViolation,
            Self::ResourceLimit(_) => RuntimeErrorKind::ResourceLimitExceeded,
            Self::MetadataConflict(_) => RuntimeErrorKind::MetadataConflict,
            Self::ExpiredHostBorrow(_) => RuntimeErrorKind::ExpiredHostBorrow,
            Self::HostBorrowConflict(_) => RuntimeErrorKind::HostBorrowConflict,
            Self::HostBorrowEscape(_) => RuntimeErrorKind::HostBorrowEscape,
            Self::InvalidReflectiveRead(_) => RuntimeErrorKind::InvalidReflectiveRead,
            Self::InvalidReflectiveWrite(_) => RuntimeErrorKind::InvalidReflectiveWrite,
            Self::HostCallFailure(_) => RuntimeErrorKind::HostCallFailure,
            Self::TypedPathValidation(_) => RuntimeErrorKind::TypedPathValidation,
            Self::ModuleValidation(_) | Self::NativeScalarMismatch => {
                RuntimeErrorKind::ModuleValidation
            }
        }
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        match self {
            Self::Trap(reason) => reason.retained_bytes(),
            Self::Index(IndexError::Provider(text)) => text.capacity(),
            Self::ExecutionPhaseViolation(text)
            | Self::ResourceLimit(text)
            | Self::MetadataConflict(text)
            | Self::ExpiredHostBorrow(text)
            | Self::HostBorrowConflict(text)
            | Self::HostBorrowEscape(text)
            | Self::TypedPathValidation(text)
            | Self::ModuleValidation(text)
            | Self::DebugObservationFailure(text)
            | Self::OwnedBackendFailure(text) => text.capacity(),
            Self::InvalidReflectiveRead(reason) | Self::InvalidReflectiveWrite(reason) => {
                reason.retained_bytes()
            }
            Self::HostCallFailure(error) => error.retained_bytes(),
            Self::InvalidVerifiedPublicTarget { path, .. } => path.capacity(),
            _ => 0,
        }
    }
}
