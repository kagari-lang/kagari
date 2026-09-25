use kagari_ir::bytecode::{BytecodeVerificationError, CallTarget, FunctionRef, ModuleSlot};
use kagari_runtime::{
    BackendDiagnostic, BackendInvocationError, RuntimeError, builtin::BuiltinError,
    host::HostError, reflection::ReflectionError,
};

#[derive(Debug, Clone)]
pub enum VmError {
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
    UnsupportedCallTarget(CallTarget),
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
