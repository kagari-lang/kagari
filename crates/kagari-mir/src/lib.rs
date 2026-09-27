//! Concrete control-flow representation and verified execution facts.
pub mod analysis;
pub mod function;
pub mod ids;
pub mod instruction;
pub mod program;
mod verify;
pub use function::{
    BasicBlock, BlockBuffer, CapturedBindingDebugBuffer, FunctionBuffer, LocalBuffer,
    MirCapturedBindingDebugInfo, MirFunction, MirFunctionDebugMetadata, MirLocal,
    MirLocalDebugBuffer, MirLocalDebugInfo, MirModule, MirModuleSlot, MirParameter, MirTemp,
    ModuleSlotBuffer, ParameterBuffer, SourceSpanBuffer, TempBuffer,
};
pub use ids::{BlockId, LocalId, ModuleSlotId, TempId};
pub use instruction::{
    AggregateFieldRef, CallTarget, Constant, Instruction, InstructionBuffer, MirValue, PathRef,
    StructFieldInit, StructFieldInitBuffer, Terminator, ValueBuffer,
};
pub use verify::{MirVerificationError, MirVerificationErrorKind, VerifiedMirModule, verify_mir};
