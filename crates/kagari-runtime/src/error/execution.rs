//! Interpreter failures use the same typed reason and trace as every other entry.
use crate::{
    backend::BackendInvocationError,
    builtin::BuiltinError,
    error::{
        RuntimeError, RuntimeErrorKind,
        fault::EngineFault,
        reason::{ErrorReason, IndexError},
        trap::TrapReason,
    },
    frame::cursor::kernel::RegionError,
    resource::ResourceState,
};

impl From<BuiltinError> for RuntimeError {
    fn from(error: BuiltinError) -> Self {
        error.into_runtime_error()
    }
}

impl From<RegionError> for RuntimeError {
    fn from(error: RegionError) -> Self {
        match error {
            RegionError::Runtime(error) => *error,
            RegionError::TypeMismatch(reason) => Self::from(TrapReason::Provider(reason.into())),
            RegionError::InvalidIndex(index) => Self::from(IndexError::InvalidIndex { index }),
        }
    }
}

impl From<BackendInvocationError> for RuntimeError {
    fn from(error: BackendInvocationError) -> Self {
        match error {
            BackendInvocationError::RuntimeFailure(error) => error,
            error => Self::new(ErrorReason::BackendInvocation(Box::new(error))),
        }
    }
}

impl RuntimeError {
    pub(crate) fn admit_execution_error(self, resources: &ResourceState) -> Self {
        if self.kind() == RuntimeErrorKind::EngineFault {
            return self;
        }
        if let Err(error) = resources.ensure_execution_allowed() {
            return error;
        }
        resources.admit_execution_error(self)
    }

    pub(crate) fn invariant_reason(&self) -> Option<EngineFault> {
        match self.reason() {
            ErrorReason::EngineFault(
                reason @ (EngineFault::VerifiedFunctionReferenceIsMissing
                | EngineFault::VerifiedBranchConditionIsNotBool
                | EngineFault::VerifiedCallTargetIsUnsupported
                | EngineFault::HostInvariant(_)),
            ) => Some(*reason),
            _ => None,
        }
    }
}
