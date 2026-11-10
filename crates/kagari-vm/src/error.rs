use kagari_bytecode::{instruction::CallTarget, verifier::BytecodeVerificationError};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    backend::BackendInvocationError,
    builtin::BuiltinError,
    error::{RuntimeError, RuntimeErrorKind, fault::EngineFault},
    error_trace::ErrorTrace,
    frame::cursor::kernel::RegionError,
    host::HostError,
    resource::ResourceState,
};

use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum VmError {
    Traced {
        error: Box<VmError>,
        trace: Arc<ErrorTrace>,
    },
    MissingFunction(String),
    MissingField(String),
    InvalidFunctionRef(FunctionRef),
    InvalidIndex(u64),
    InvalidBranchCondition,
    HostError(HostError),
    RuntimeError(RuntimeError),
    BytecodeVerification(BytecodeVerificationError),
    JitInvocation(BackendInvocationError),
    Trap(&'static str),
    TypeMismatch(&'static str),
    UnsupportedCallTarget(Box<CallTarget>),
    UnsupportedInstruction(&'static str),
}

impl From<BuiltinError> for VmError {
    fn from(error: BuiltinError) -> Self {
        Self::RuntimeError(error.into_runtime_error())
    }
}

impl From<RegionError> for VmError {
    fn from(error: RegionError) -> Self {
        match error {
            RegionError::Runtime(error) => Self::RuntimeError(*error),
            RegionError::TypeMismatch(reason) => Self::TypeMismatch(reason),
            RegionError::InvalidIndex(index) => Self::InvalidIndex(index),
        }
    }
}

impl From<RuntimeError> for VmError {
    fn from(error: RuntimeError) -> Self {
        Self::RuntimeError(error)
    }
}

impl VmError {
    pub fn cause(&self) -> &Self {
        match self {
            Self::Traced { error, .. } => error.cause(),
            _ => self,
        }
    }

    pub fn trace(&self) -> Option<&Arc<ErrorTrace>> {
        match self {
            Self::Traced { trace, .. } => Some(trace),
            Self::RuntimeError(error) => error.trace(),
            Self::HostError(error) => error.trace(),
            _ => None,
        }
    }

    pub(crate) fn with_trace(self, trace: Arc<ErrorTrace>) -> Self {
        if self
            .trace()
            .is_some_and(|previous| !previous.frames.is_empty())
        {
            return self;
        }
        match self {
            Self::RuntimeError(error) => Self::RuntimeError(error.with_trace(trace)),
            Self::Traced { error, .. } => error.with_trace(trace),
            error => Self::Traced {
                error: Box::new(error),
                trace,
            },
        }
    }

    pub(crate) fn admit_execution_error(self, resources: &ResourceState) -> Self {
        if matches!(self.cause(), Self::RuntimeError(error) if error.kind() == RuntimeErrorKind::EngineFault)
        {
            return self;
        }
        // A secondary failure cannot replace root termination; quarantine still wins.
        if let Err(error) = resources.ensure_execution_allowed() {
            return Self::RuntimeError(error);
        }
        if let Self::RuntimeError(error) = self.cause() {
            let mut error = error.clone();
            if let Some(trace) = self.trace() {
                error = error.with_trace(trace.clone());
            }
            return Self::RuntimeError(resources.admit_execution_error(error));
        }
        self
    }

    pub(crate) fn invariant_reason(&self) -> Option<EngineFault> {
        match self {
            Self::InvalidFunctionRef(_) => Some(EngineFault::VerifiedFunctionReferenceIsMissing),
            Self::InvalidBranchCondition => Some(EngineFault::VerifiedBranchConditionIsNotBool),
            Self::UnsupportedCallTarget(_) => Some(EngineFault::VerifiedCallTargetIsUnsupported),
            Self::UnsupportedInstruction(reason) => Some(EngineFault::HostInvariant(reason)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_runtime::error::reason::ErrorReason;
    use kagari_types::scalar::error::ScalarError;

    #[test]
    fn builtin_errors_enter_the_vm_with_their_original_trace_and_reason() {
        let trace = Arc::new(ErrorTrace {
            incomplete: true,
            ..Default::default()
        });
        let error = VmError::from(BuiltinError::from(
            RuntimeError::from(ScalarError::InvalidOperandDomain).with_trace(trace.clone()),
        ));
        assert!(Arc::ptr_eq(error.trace().unwrap(), &trace));
        let VmError::RuntimeError(error) = error.cause() else {
            panic!("one Runtime error route")
        };
        assert_eq!(
            error.reason(),
            &ErrorReason::Scalar(ScalarError::InvalidOperandDomain)
        );
    }
}
