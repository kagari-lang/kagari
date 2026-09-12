pub use kagari_hir::builtin::surface::StandardIntrinsic;

pub mod abi;
pub mod contracts;
pub mod function;
pub mod ids;
pub mod instruction;
pub mod layout;
pub use layout::{StructFieldLayout, StructLayout};
pub mod types;
mod verify;

pub use verify::{IrVerificationError, IrVerificationErrorKind, VerifiedIrModule, verify_ir};

pub use abi::{
    ConstAbi, FieldAbi, FunctionAbi, InterfaceTableAbi, ModuleAbi, ParameterAbi, PublicAbiItem,
    PublicAbiItemBuffer, TraitAbi, TypeAbi, TypeAbiKind, VariantAbi,
};
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
