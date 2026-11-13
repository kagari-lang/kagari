//! Failures at heap ownership, resource and unsafe-provider boundaries.

use std::collections::TryReserveError;
use thiserror::Error;

/// A rejected operation never grants access to an unvalidated object address.
/// Used for admission/resource failures and fatal reports at collection/provider
/// boundaries. An Err does not imply the heap is recoverable: Faulted heaps only
/// allow shutdown. Ordinary typed field access/traversal instead panic on contract
/// violations; dynamic layout admission and downcasts retain explicit errors.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum GcError {
    #[error("the handle belongs to another heap")]
    ForeignHeap,
    #[error("the root lease is expired or invalid")]
    InvalidRoot,
    #[error("the representation does not match the admitted type or layout")]
    Representation,
    #[error("invalid managed storage: {0}")]
    Storage(&'static str),
    #[error("the heap is faulted")]
    Faulted,
    #[error("the heap is shut down")]
    Closed,
    #[error("resource limit: {0}")]
    Limit(&'static str),
    #[error("allocation failed")]
    Allocation,
    #[error("provider contract failed: {0}")]
    Provider(&'static str),
    #[error("index is outside the container")]
    Index,
}

impl From<TryReserveError> for GcError {
    fn from(_: TryReserveError) -> Self {
        Self::Allocation
    }
}
