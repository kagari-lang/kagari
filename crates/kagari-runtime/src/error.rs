//! Runtime failures keep typed reasons separate from owned diagnostic context.
pub mod context;
mod execution;
pub mod fault;
pub mod reason;
pub mod trap;

use crate::{
    error::{
        context::{ErrorContext, ErrorLocation},
        fault::EngineFault,
        reason::{
            CancellationReason, ErrorReason, IndexError, ReflectiveFailure, StaleHandleReason,
        },
        trap::TrapReason,
    },
    error_trace::{ErrorTrace, asynchronous::AsyncBoundary},
    host::HostError,
    range::RangeError,
    task::TaskFailureOrigin,
};
use kagari_types::scalar::error::ScalarError;
use std::{
    error::Error,
    fmt::{self, Display},
    mem,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    UnsupportedExecution,
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
    StackOverflow,
    AllocationFailure,
    MetadataConflict,
    StaleHandle,
}

impl RuntimeErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::UnsupportedExecution => "KG_RUNTIME_UNSUPPORTED_EXECUTION",
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
            Self::StackOverflow => "KG_RUNTIME_STACK_OVERFLOW",
            Self::AllocationFailure => "KG_RUNTIME_ALLOCATION_FAILURE",
            Self::MetadataConflict => "KG_RUNTIME_METADATA_CONFLICT",
            Self::StaleHandle => "KG_RUNTIME_STALE_HANDLE",
        }
    }
}

/// An execution failure with one authoritative reason and optional cold context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    reason: ErrorReason,
    context: Option<Box<ErrorContext>>,
}

impl RuntimeError {
    /// Construct a diagnostic without formatting or allocating execution context.
    pub fn new(reason: ErrorReason) -> Self {
        Self {
            reason,
            context: None,
        }
    }

    /// The typed reason is preserved when positions, traces or task origins are added.
    pub fn reason(&self) -> &ErrorReason {
        &self.reason
    }

    /// Owned storage attributed to retained task failures. Shared trace metadata
    /// remains excluded even when several failures retain it.
    pub(crate) fn retained_bytes(&self) -> usize {
        self.reason.retained_bytes()
            + self.context.as_ref().map_or(0, |context| {
                mem::size_of::<ErrorContext>()
                    + context.locations.capacity() * mem::size_of::<ErrorLocation>()
                    + context
                        .locations
                        .iter()
                        .map(ErrorLocation::retained_bytes)
                        .sum::<usize>()
            })
    }

    pub fn execution_phase_violation(operation: impl Into<String>) -> Self {
        Self::new(ErrorReason::ExecutionPhaseViolation(operation.into()))
    }

    pub fn resource_limit(limit: impl Into<String>) -> Self {
        Self::new(ErrorReason::ResourceLimit(limit.into()))
    }

    pub fn metadata_conflict(name: impl Into<String>) -> Self {
        Self::new(ErrorReason::MetadataConflict(name.into()))
    }

    pub fn expired_host_borrow(detail: impl Into<String>) -> Self {
        Self::new(ErrorReason::ExpiredHostBorrow(detail.into()))
    }

    pub fn host_borrow_conflict(detail: impl Into<String>) -> Self {
        Self::new(ErrorReason::HostBorrowConflict(detail.into()))
    }

    pub fn host_borrow_escape(detail: impl Into<String>) -> Self {
        Self::new(ErrorReason::HostBorrowEscape(detail.into()))
    }

    pub fn typed_path_validation(detail: impl Into<String>) -> Self {
        Self::new(ErrorReason::TypedPathValidation(detail.into()))
    }

    pub fn module_validation(detail: impl Into<String>) -> Self {
        Self::new(ErrorReason::ModuleValidation(detail.into()))
    }

    pub fn invalid_reflective_read(detail: impl Into<String>) -> Self {
        Self::new(ErrorReason::InvalidReflectiveRead(
            ReflectiveFailure::Detail(detail.into()),
        ))
    }

    pub fn invalid_reflective_write(detail: impl Into<String>) -> Self {
        Self::new(ErrorReason::InvalidReflectiveWrite(
            ReflectiveFailure::Detail(detail.into()),
        ))
    }

    pub fn host_call_failure(detail: impl Into<String>) -> Self {
        Self::from_host(HostError::new(detail))
    }

    /// Preserve the Host diagnostic and its original trace without flattening it.
    pub fn from_host(error: HostError) -> Self {
        let trace = error.trace().cloned();
        let mut failure = Self::new(ErrorReason::HostCallFailure(error));
        if let Some(trace) = trace {
            failure = failure.with_trace(trace);
        }
        failure
    }

    pub(crate) fn into_reflective_read(self) -> Self {
        let trace = self.trace().cloned();
        let origin = self.task_origin();
        let mut error = Self::new(ErrorReason::InvalidReflectiveRead(
            ReflectiveFailure::Cause(Box::new(self)),
        ));
        if let Some(trace) = trace {
            error = error.with_trace(trace);
        }
        if let Some(origin) = origin {
            error = error.with_task_origin(origin);
        }
        error
    }

    pub(crate) fn into_reflective_write(self) -> Self {
        let trace = self.trace().cloned();
        let origin = self.task_origin();
        let mut error = Self::new(ErrorReason::InvalidReflectiveWrite(
            ReflectiveFailure::Cause(Box::new(self)),
        ));
        if let Some(trace) = trace {
            error = error.with_trace(trace);
        }
        if let Some(origin) = origin {
            error = error.with_task_origin(origin);
        }
        error
    }

    /// Attach one owned position on failure; successful operations allocate none.
    pub fn at(mut self, location: ErrorLocation) -> Self {
        self.context
            .get_or_insert_with(Default::default)
            .locations
            .push(location);
        self
    }

    /// Positions in attachment order, from inner to outer.
    pub fn locations(&self) -> &[ErrorLocation] {
        self.context
            .as_ref()
            .map_or(&[], |context| context.locations.as_slice())
    }

    pub fn trace(&self) -> Option<&Arc<ErrorTrace>> {
        self.context
            .as_ref()
            .and_then(|context| context.trace.as_ref())
    }

    pub fn task_origin(&self) -> Option<TaskFailureOrigin> {
        self.context
            .as_ref()
            .and_then(|context| context.task_origin)
    }

    pub(crate) fn with_task_origin(mut self, origin: TaskFailureOrigin) -> Self {
        self.context
            .get_or_insert_with(Default::default)
            .task_origin
            .get_or_insert(origin);
        self
    }

    pub(crate) fn with_async_boundary(mut self, boundary: AsyncBoundary) -> Self {
        let trace = self
            .context
            .get_or_insert_with(Default::default)
            .trace
            .get_or_insert_with(|| Arc::new(ErrorTrace::default()));
        Arc::make_mut(trace).append_async(boundary);
        self
    }

    pub fn with_trace(mut self, trace: Arc<ErrorTrace>) -> Self {
        let context = self.context.get_or_insert_with(Default::default);
        if let Some(previous) = &mut context.trace
            && previous.frames.is_empty()
            && !previous.async_boundaries.is_empty()
        {
            let previous = Arc::make_mut(previous);
            previous.frames = trace.frames.clone();
            previous.omitted_frames = trace.omitted_frames;
            previous.incomplete |= trace.incomplete;
            return self;
        }
        if context
            .trace
            .as_ref()
            .is_none_or(|previous| previous.frames.is_empty())
        {
            context.trace = Some(trace);
        }
        self
    }

    pub fn kind(&self) -> RuntimeErrorKind {
        self.reason.kind()
    }

    pub fn code(&self) -> &'static str {
        self.kind().code()
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for location in self.locations().iter().rev() {
            write!(f, "{location}: ")?;
        }
        Display::fmt(&self.reason, f)
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.reason)
    }
}

impl From<RangeError> for RuntimeError {
    fn from(reason: RangeError) -> Self {
        match reason {
            RangeError::InvalidShape => Self::from(TrapReason::InvalidRangeValue),
            RangeError::Endpoint(error) => Self::from(error),
        }
    }
}

impl From<ScalarError> for RuntimeError {
    fn from(reason: ScalarError) -> Self {
        Self::new(reason.into())
    }
}

impl From<TrapReason> for RuntimeError {
    fn from(reason: TrapReason) -> Self {
        Self::new(reason.into())
    }
}

impl From<EngineFault> for RuntimeError {
    fn from(reason: EngineFault) -> Self {
        Self::new(reason.into())
    }
}

impl From<CancellationReason> for RuntimeError {
    fn from(reason: CancellationReason) -> Self {
        Self::new(reason.into())
    }
}

impl From<StaleHandleReason> for RuntimeError {
    fn from(reason: StaleHandleReason) -> Self {
        Self::new(reason.into())
    }
}

impl From<IndexError> for RuntimeError {
    fn from(reason: IndexError) -> Self {
        Self::new(reason.into())
    }
}

#[cfg(test)]
mod tests;
