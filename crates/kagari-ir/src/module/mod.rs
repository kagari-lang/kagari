pub use kagari_hir::builtin::BuiltinMethod;

pub mod function;
pub mod ids;
pub mod instruction;
pub mod types;

pub use function::{
    BasicBlock, BlockBuffer, CapturedBindingDebugBuffer, FunctionBuffer,
    IrCapturedBindingDebugInfo, IrFunction, IrFunctionDebugMetadata, IrLocal, IrLocalDebugBuffer,
    IrLocalDebugInfo, IrModule, IrModuleSlot, IrParameter, IrTemp, LocalBuffer, ModuleSlotBuffer,
    ParameterBuffer, SourceSpanBuffer, TempBuffer,
};
pub use ids::{BlockId, LocalId, ModuleSlotId, TempId};
pub use instruction::{
    AggregateFieldRef, BinaryOp, CallTarget, Constant, EffectSet, Instruction, InstructionBuffer,
    IrValue, PathRef, StructFieldInit, StructFieldInitBuffer, Terminator, UnaryOp, ValueBuffer,
};
pub use types::ValueType;
