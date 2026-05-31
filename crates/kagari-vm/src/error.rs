use kagari_ir::bytecode::{
    BytecodeVerificationError, CallTarget, FunctionRef, JumpTarget, LocalSlot, ModuleSlot, Register,
};
use kagari_runtime::{
    RuntimeError, builtin::BuiltinError, host::HostError, reflection::ReflectionError,
};

#[derive(Debug, Clone)]
pub enum VmError {
    MissingFunction(String),
    MissingField(String),
    InvalidFunctionRef(FunctionRef),
    InvalidFrameArity {
        function: FunctionRef,
        expected: usize,
        found: usize,
    },
    InvalidJumpTarget(JumpTarget),
    InvalidRegister(Register),
    InvalidLocal(LocalSlot),
    InvalidModuleSlot(ModuleSlot),
    ImmutableModuleSlot(ModuleSlot),
    InvalidIndex(usize),
    InvalidBranchCondition,
    HostError(HostError),
    BuiltinError(BuiltinError),
    ReflectionError(ReflectionError),
    RuntimeError(RuntimeError),
    BytecodeVerification(BytecodeVerificationError),
    Trap(&'static str),
    TypeMismatch(&'static str),
    UnsupportedCallTarget(CallTarget),
    UnsupportedInstruction(&'static str),
}
