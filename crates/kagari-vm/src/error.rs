use kagari_ir::bytecode::{BytecodeVerificationError, CallTarget, FunctionRef, ModuleSlot};
use kagari_runtime::{
    BackendDiagnostic, BackendInvocationError, RuntimeError, builtin::BuiltinError,
    host::HostError, reflection::ReflectionError,
};

#[derive(Debug, Clone)]
pub enum VmError {
    Traced {
        error: Box<VmError>,
        trace: std::sync::Arc<kagari_runtime::ErrorTrace>,
    },
    MissingFunction(String),
    AmbiguousFunction(String),
    MissingField(String),
    InvalidFunctionRef(FunctionRef),
    InvalidModuleSlot(ModuleSlot),
    ImmutableModuleSlot(ModuleSlot),
    InvalidIndex(usize),
    InvalidBranchCondition,
    HostError(HostError),
    BuiltinError(BuiltinError),
    ReflectionError(ReflectionError),
    RuntimeError(RuntimeError),
    BytecodeVerification(BytecodeVerificationError),
    JitBackend(Vec<BackendDiagnostic>),
    JitInvocation(BackendInvocationError),
    Trap(&'static str),
    TypeMismatch(&'static str),
    UnsupportedCallTarget(Box<CallTarget>),
    UnsupportedInstruction(&'static str),
}

impl From<BuiltinError> for VmError {
    fn from(error: BuiltinError) -> Self {
        if error.kind() == kagari_runtime::RuntimeErrorKind::ScriptTrap {
            Self::BuiltinError(error)
        } else {
            Self::RuntimeError(error.into_runtime_error())
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
    pub fn trace(&self) -> Option<&std::sync::Arc<kagari_runtime::ErrorTrace>> {
        match self {
            Self::Traced { trace, .. } => Some(trace),
            Self::RuntimeError(error) => error.trace(),
            Self::HostError(error) => error.trace(),
            _ => None,
        }
    }
    pub(crate) fn with_trace(self, trace: std::sync::Arc<kagari_runtime::ErrorTrace>) -> Self {
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

    pub(crate) fn invariant_reason(&self) -> Option<&'static str> {
        match self {
            Self::InvalidFunctionRef(_) => Some("verified function reference is missing"),
            Self::InvalidModuleSlot(_) => Some("verified module slot is missing"),
            Self::InvalidBranchCondition => Some("verified branch condition is not bool"),
            Self::UnsupportedCallTarget(_) => Some("verified call target is unsupported"),
            Self::UnsupportedInstruction(reason) => Some(reason),
            _ => None,
        }
    }
}
