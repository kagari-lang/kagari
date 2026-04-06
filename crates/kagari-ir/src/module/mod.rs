pub mod function;
pub mod ids;
pub mod instruction;
pub mod types;

pub use function::{
    BasicBlock, BlockBuffer, FunctionBuffer, IrFunction, IrLocal, IrModule, IrParameter, IrTemp,
    LocalBuffer, ParameterBuffer, TempBuffer,
};
pub use ids::{BlockId, LocalId, TempId};
pub use instruction::{
    BinaryOp, CallTarget, Constant, Instruction, InstructionBuffer, StructFieldInit,
    StructFieldInitBuffer, TempIdBuffer, Terminator, UnaryOp,
};
pub use types::ValueType;
