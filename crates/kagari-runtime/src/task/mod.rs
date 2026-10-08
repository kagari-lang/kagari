//! Scope-owned executions, durable readiness and GC-traced terminal Task values.
mod admission;
mod cleanup;
pub mod control;
mod dependencies;
pub mod drive;
mod outcome;
pub(crate) mod payload;
pub(crate) mod store;
pub(crate) mod waiting;

use crate::{error::RuntimeError, gc::roots::RootedValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub(crate) store::Identity);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub(crate) store::Identity);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    ScopeClosed,
    CapacityExceeded,
    DispatchUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CancellationCause {
    Explicit = 1,
    ScopeClose = 2,
    OwnerDrop = 3,
    DispatchFailure = 4,
    Dependency = 5,
    RuntimeShutdown = 6,
}

#[derive(Debug, Clone)]
pub struct TaskFailure {
    pub error: RuntimeError,
    pub source_task: TaskId,
    pub cancellation: Option<CancellationCause>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskFailureOrigin {
    pub task: TaskId,
    pub cancellation: Option<CancellationCause>,
}

#[derive(Debug)]
pub struct TaskReport {
    pub task: TaskId,
    pub scope: ScopeId,
    pub outcome: Result<RootedValue, TaskFailure>,
}
