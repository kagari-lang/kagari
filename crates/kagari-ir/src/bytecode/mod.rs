mod instruction;
mod lower;
mod module;

pub use instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FunctionRef, JumpTarget, LocalSlot,
    ModuleSlot, Register, RuntimeHelper, StructFieldInit, UnaryOp,
};
pub use lower::{BytecodeLoweringError, lower_to_bytecode};
pub use module::{
    BytecodeFunction, BytecodeFunctionBuffer, BytecodeInstructionBuffer, BytecodeModule,
    BytecodeModuleSlot, BytecodeModuleSlotBuffer,
};
