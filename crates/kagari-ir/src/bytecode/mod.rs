pub use kagari_hir::builtin::BuiltinMethod;

mod instruction;
mod lower;
mod module;
mod verifier;

pub use instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FunctionRef, JumpTarget, LocalSlot,
    ModuleSlot, Register, RuntimeHelper, StructFieldInit, UnaryOp,
};
pub use lower::{BytecodeLoweringError, lower_to_bytecode};
pub use module::{
    BytecodeFunction, BytecodeFunctionBuffer, BytecodeInstructionBuffer, BytecodeModule,
    BytecodeModuleSlot, BytecodeModuleSlotBuffer, BytecodeTypeTable, ConstantPool,
    ControlFlowTargetBuffer, FunctionMetadata, FunctionRecord, FunctionTable, PublicItemRecord,
    PublicItemTable, TypeLayoutBuffer,
};
pub use verifier::{BytecodeVerificationError, verify_module};
