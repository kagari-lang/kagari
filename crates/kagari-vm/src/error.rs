use kagari_ir::bytecode::{CallTarget, FunctionRef, JumpTarget, LocalSlot, ModuleSlot, Register};
use kagari_runtime::{host::HostError, reflection::ReflectionError};

#[derive(Debug, Clone)]
pub enum VmError {
    MissingFunction(String),
    MissingField(String),
    InvalidFunctionRef(FunctionRef),
    InvalidJumpTarget(JumpTarget),
    InvalidRegister(Register),
    InvalidLocal(LocalSlot),
    InvalidModuleSlot(ModuleSlot),
    ImmutableModuleSlot(ModuleSlot),
    InvalidIndex(usize),
    InvalidBranchCondition,
    HostError(HostError),
    ReflectionError(ReflectionError),
    TypeMismatch(&'static str),
    UnsupportedCallTarget(CallTarget),
    UnsupportedInstruction(&'static str),
}
