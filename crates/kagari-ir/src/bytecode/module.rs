use crate::{
    bytecode::instruction::{
        BytecodeInstruction, ConstantOperand, FunctionRef, JumpTarget, LocalSlot, PathId, Register,
    },
    module::{ConcreteFunctionIdentity, EffectSet, PublicAbiItem, TraitContract, ValueType},
};
use kagari_common::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BytecodeModule {
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub dependencies: Vec<super::ModuleRef>,
    pub host_interface: kagari_common::host_interface::HostInterface,
    pub identity: kagari_common::identity::ModuleIdentity,
    pub source_name: String,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub module_slots: BytecodeModuleSlotBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub constants: ConstantPool,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub types: BytecodeTypeTable,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub structures: Vec<crate::module::StructLayout>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub enumerations: Vec<crate::module::EnumLayout>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub interface_tables: Vec<InterfaceTableRecord>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub paths: PathTable,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub function_table: FunctionTable,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub public_items: PublicItemTable,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub trait_contracts: Vec<TraitContract>,
    #[serde(deserialize_with = "crate::decode_limits::functions")]
    pub functions: BytecodeFunctionBuffer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BytecodeModuleSlot {
    pub name: String,
    pub ty: ValueType,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathRecord {
    pub contract_fingerprint: u64,
    pub id: PathId,
    pub root_ty: ValueType,
    pub result_ty: ValueType,
    pub read_only: bool,
    pub debug_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BytecodeFunction {
    pub id: FunctionRef,
    pub identity: Option<ConcreteFunctionIdentity>,
    pub name: String,
    pub parameter_count: u16,
    pub register_count: u16,
    pub local_count: u16,
    pub metadata: FunctionMetadata,
    #[serde(deserialize_with = "crate::decode_limits::instructions")]
    pub instructions: BytecodeInstructionBuffer,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionMetadata {
    pub semantic: crate::module::function::SemanticSlots,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub locals: TypeLayoutBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub registers: TypeLayoutBuffer,
    pub roots: RootSlotLayout,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub control_flow_targets: ControlFlowTargetBuffer,
    pub effects: EffectSet,
    pub debug: BytecodeDebugMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionRecord {
    pub id: FunctionRef,
    pub identity: Option<ConcreteFunctionIdentity>,
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    pub effects: EffectSet,
}

/// Conservative frame roots, indexed by verified local and register slots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootSlotLayout {
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub locals: Vec<LocalSlot>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub registers: Vec<Register>,
}

impl RootSlotLayout {
    pub fn from_types(locals: &[ValueType], registers: &[ValueType]) -> Self {
        Self {
            locals: locals
                .iter()
                .enumerate()
                .filter(|(_, ty)| **ty == ValueType::HeapObject)
                .map(|(index, _)| LocalSlot::new(index))
                .collect(),
            registers: registers
                .iter()
                .enumerate()
                .filter(|(_, ty)| **ty == ValueType::HeapObject)
                .map(|(index, _)| Register::new(index))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceTableRecord {
    /// Ordered impl arguments. An empty record for a generic template retains
    /// static method instances and cannot be selected by MakeInterface.
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<crate::module::abi::AbiType>,
    pub declaration: kagari_common::identity::DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub methods: Vec<InterfaceMethodSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceMethodSlot {
    pub method: kagari_common::identity::DefinitionId,
    pub function: FunctionRef,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BytecodeDebugMetadata {
    pub source_uri: Option<String>,
    pub source_module: Option<super::ModuleRef>,
    pub function_span: Span,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub source_spans: InstructionSourceSpanBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub line_table: LineTableBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub safe_debug_points: SafeDebugPointBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub local_live_ranges: LocalLiveRangeBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
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
    /// First instruction offset where the initialized binding is visible.
    pub start: usize,
    /// Exclusive end offset.
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
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub params: TypeLayoutBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub locals: TypeLayoutBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub registers: TypeLayoutBuffer,
}

pub type BytecodeFunctionBuffer = Vec<BytecodeFunction>;
pub type BytecodeInstructionBuffer = Vec<BytecodeInstruction>;
pub type BytecodeModuleSlotBuffer = Vec<BytecodeModuleSlot>;
pub type ConstantPool = Vec<ConstantOperand>;
pub type BytecodeTypeTable = Vec<ValueType>;

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
