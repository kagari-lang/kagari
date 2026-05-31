pub use kagari_hir::builtin::BuiltinMethod;

mod instruction;
mod lower;
mod module;
mod verifier;

pub use instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FieldId, FunctionRef, JumpTarget,
    LocalSlot, ModuleSlot, PathId, Register, RuntimeHelper, StructFieldInit, UnaryOp,
};
pub use lower::{BytecodeLoweringError, lower_to_bytecode};
pub use module::{
    BytecodeFunction, BytecodeFunctionBuffer, BytecodeInstructionBuffer, BytecodeModule,
    BytecodeModuleSlot, BytecodeModuleSlotBuffer, BytecodeTypeTable, ConstantPool,
    ControlFlowTargetBuffer, FieldRecord, FieldTable, FunctionMetadata, FunctionRecord,
    FunctionTable, PathRecord, PathTable, PublicItemRecord, PublicItemTable, TypeLayoutBuffer,
};
pub use verifier::{BytecodeVerificationError, verify_module};
