use crate::{
    bytecode::instruction::{
        BytecodeInstruction, ConstantOperand, FieldId, FunctionRef, JumpTarget, LocalSlot, PathId,
    },
    module::{EffectSet, PublicAbiItem, ValueType},
};
use kagari_common::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BytecodeModule {
    pub module_init: Option<FunctionRef>,
    pub module_slots: BytecodeModuleSlotBuffer,
    pub constants: ConstantPool,
    pub types: BytecodeTypeTable,
    pub fields: FieldTable,
    pub paths: PathTable,
    pub function_table: FunctionTable,
    pub public_items: PublicItemTable,
    pub functions: BytecodeFunctionBuffer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BytecodeModuleSlot {
    pub name: String,
    pub ty: ValueType,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldRecord {
    pub id: FieldId,
    pub owner: String,
    pub name: String,
    pub ty: ValueType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathRecord {
    pub id: PathId,
    pub root_ty: ValueType,
    pub result_ty: ValueType,
    pub read_only: bool,
    pub debug_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BytecodeFunction {
    pub id: FunctionRef,
    pub name: String,
    pub parameter_count: u16,
    pub register_count: u16,
    pub local_count: u16,
    pub metadata: FunctionMetadata,
    pub instructions: BytecodeInstructionBuffer,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionMetadata {
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    pub locals: TypeLayoutBuffer,
    pub registers: TypeLayoutBuffer,
    pub control_flow_targets: ControlFlowTargetBuffer,
    pub effects: EffectSet,
    pub debug: BytecodeDebugMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionRecord {
    pub id: FunctionRef,
    pub name: String,
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    pub effects: EffectSet,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BytecodeDebugMetadata {
    pub function_span: Span,
    pub source_spans: InstructionSourceSpanBuffer,
    pub line_table: LineTableBuffer,
    pub safe_debug_points: SafeDebugPointBuffer,
    pub local_live_ranges: LocalLiveRangeBuffer,
    pub captured_bindings: CapturedBindingDebugBuffer,
    pub frame_layout: FrameLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionSourceSpan {
    pub instruction_offset: usize,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineTableEntry {
    pub instruction_offset: usize,
    pub source_offset: usize,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeDebugPoint {
    pub id: DebugPointId,
    pub instruction_offset: usize,
    pub span: Span,
    pub kind: SafeDebugPointKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebugPointId(u32);

impl DebugPointId {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafeDebugPointKind {
    FunctionEntry,
    Statement,
    BranchTarget,
    CallBoundary,
    FunctionReturn,
    Trap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLiveRange {
    pub local: LocalSlot,
    pub name: String,
    pub span: Span,
    pub start: usize,
    pub end: usize,
    pub ty: ValueType,
    pub is_parameter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedBindingDebugInfo {
    pub name: String,
    pub span: Span,
    pub ty: ValueType,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameLayout {
    pub params: TypeLayoutBuffer,
    pub locals: TypeLayoutBuffer,
    pub registers: TypeLayoutBuffer,
}

pub type BytecodeFunctionBuffer = Vec<BytecodeFunction>;
pub type BytecodeInstructionBuffer = Vec<BytecodeInstruction>;
pub type BytecodeModuleSlotBuffer = Vec<BytecodeModuleSlot>;
pub type ConstantPool = Vec<ConstantOperand>;
pub type BytecodeTypeTable = Vec<ValueType>;
pub type FieldTable = Vec<FieldRecord>;
pub type PathTable = Vec<PathRecord>;
pub type FunctionTable = Vec<FunctionRecord>;
pub type PublicItemRecord = PublicAbiItem;
pub type PublicItemTable = Vec<PublicAbiItem>;
pub type TypeLayoutBuffer = Vec<ValueType>;
pub type ControlFlowTargetBuffer = Vec<JumpTarget>;
pub type InstructionSourceSpanBuffer = Vec<InstructionSourceSpan>;
pub type LineTableBuffer = Vec<LineTableEntry>;
pub type SafeDebugPointBuffer = Vec<SafeDebugPoint>;
pub type LocalLiveRangeBuffer = Vec<LocalLiveRange>;
pub type CapturedBindingDebugBuffer = Vec<CapturedBindingDebugInfo>;
