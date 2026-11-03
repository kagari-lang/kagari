//! Physical error conversion owned by memory integration, not script execution.

use crate::error::{RuntimeError, RuntimeErrorKind};
use kagari_gc::error::GcError;

pub fn failure(error: GcError) -> RuntimeError {
    let kind = match error {
        GcError::Allocation | GcError::Limit(_) => RuntimeErrorKind::ResourceLimitExceeded,
        GcError::ForeignHeap | GcError::InvalidRoot => RuntimeErrorKind::StaleHandle,
        GcError::Index => RuntimeErrorKind::IndexOutOfBounds,
        GcError::Representation => RuntimeErrorKind::ModuleValidation,
        GcError::Storage(_) => RuntimeErrorKind::HostCallFailure,
        GcError::Faulted | GcError::Closed | GcError::Provider(_) => RuntimeErrorKind::EngineFault,
    };
    RuntimeError::new(kind, error.to_string())
}
