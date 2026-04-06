pub mod function;
pub mod ids;
pub mod instruction;
pub mod types;

pub use function::{
    BasicBlock, BlockBuffer, FunctionBuffer, IrFunction, IrLocal, IrModule, IrModuleSlot,
    IrParameter, IrTemp, LocalBuffer, ModuleSlotBuffer, ParameterBuffer, TempBuffer,
};
pub use ids::{BlockId, LocalId, ModuleSlotId, TempId};
pub use instruction::{
    BinaryOp, CallTarget, Constant, Instruction, InstructionBuffer, StructFieldInit,
    StructFieldInitBuffer, TempIdBuffer, Terminator, UnaryOp,
};
pub use types::ValueType;
