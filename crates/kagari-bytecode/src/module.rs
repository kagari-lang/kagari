use crate::{
    instruction::{
        BytecodeInstruction, ConstantOperand, JumpTarget, LocalSlot, NativeImportId, PathId,
        Register,
    },
    program::ModuleRef,
};
use kagari_common::identity::reference::DefinitionReference;
use kagari_common::{
    host_interface::HostInterface,
    identity::{DefinitionPath, ModuleIdentity},
    span::Span,
};
use serde::{Deserialize, Serialize};
use {
    kagari_abi::representation::ValueType,
    kagari_contract::{
        effects::EffectSet,
        ids::{DebugPointId, FunctionRef},
        layout::{EnumLayout, StructLayout},
        native_import::NativeImport,
        slots::SemanticSlots,
        types::{
            ConcreteFunctionIdentity, NativeDeclaration, NominalTy, PublicItem, TraitContract, Ty,
        },
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct BytecodeModule<I = DefinitionPath> {
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub dependencies: Vec<ModuleRef>,
    pub host_interface: HostInterface<I>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub native_imports: Vec<NativeImport<I>>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub native_declarations: Vec<NativeDeclaration<I>>,
    pub identity: ModuleIdentity,
    pub source_name: String,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub module_slots: BytecodeModuleSlotBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub constants: ConstantPool,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub types: BytecodeTypeTable,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub structures: Vec<StructLayout<I>>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub enumerations: Vec<EnumLayout<I>>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub interface_tables: Vec<InterfaceTableRecord<I>>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub paths: PathTable,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub function_table: FunctionTable<I>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub public_items: PublicItemTable<I>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    pub trait_contracts: Vec<TraitContract<I>>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::functions")]
    pub functions: BytecodeFunctionBuffer<I>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct BytecodeFunction<I = DefinitionPath> {
    pub id: FunctionRef,
    pub identity: Option<ConcreteFunctionIdentity<I>>,
    pub name: String,
    pub parameter_count: u16,
    pub register_count: u16,
    pub local_count: u16,
    pub metadata: FunctionMetadata<I>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::instructions")]
    pub instructions: BytecodeInstructionBuffer<I>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct FunctionMetadata<I = DefinitionPath> {
    /// Explicit logical charges in emission order; one entry per executable point.
    pub semantic: SemanticSlots<I>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub locals: TypeLayoutBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub registers: TypeLayoutBuffer,
    pub roots: RootSlotLayout,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub control_flow_targets: ControlFlowTargetBuffer,
    pub effects: EffectSet,
    pub debug: BytecodeDebugMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct FunctionRecord<I = DefinitionPath> {
    pub id: FunctionRef,
    pub identity: Option<ConcreteFunctionIdentity<I>>,
    pub name: String,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    pub effects: EffectSet,
}

/// Conservative frame roots, indexed by verified local and register slots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootSlotLayout {
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub locals: Vec<LocalSlot>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub registers: Vec<Register>,
}

impl RootSlotLayout {
    pub fn from_types(locals: &[ValueType], registers: &[ValueType]) -> Self {
        Self {
            locals: locals
                .iter()
                .enumerate()
                .filter(|(_, ty)| ty.may_contain_gc_reference())
                .map(|(index, _)| LocalSlot::new(index))
                .collect(),
            registers: registers
                .iter()
                .enumerate()
                .filter(|(_, ty)| ty.may_contain_gc_reference())
                .map(|(index, _)| Register::new(index))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct InterfaceTableRecord<I = DefinitionPath> {
    /// Optional dynamic surface that hides a concrete associated iterator.
    pub view: Option<InterfaceViewRecord<I>>,
    /// Ordered impl arguments. An empty record for a generic template retains
    /// static method instances and cannot be selected by MakeInterface.
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    pub arguments: Vec<Ty<I>>,
    pub declaration: I,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub methods: Vec<InterfaceMethodSlot<I>>,
    /// Preselected ancestor tables, including their receiver argument mappings.
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    pub parents: Vec<InterfaceParentRecord<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct InterfaceParentRecord<I = DefinitionPath> {
    pub interface: NominalTy<I>,
    pub implementation: ConcreteFunctionIdentity<I>,
    pub view: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct InterfaceViewRecord<I = DefinitionPath> {
    pub interface: NominalTy<I>,
    /// Only changed return representations have an adapter; raw slots remain exact.
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub results: Vec<InterfaceResultAdapter<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct InterfaceResultAdapter<I = DefinitionPath> {
    pub method: I,
    pub implementation: ConcreteFunctionIdentity<I>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct InterfaceMethodSlot<I = DefinitionPath> {
    pub method: I,
    pub target: CallableTarget,
    /// Arguments for the target's shared entry, expressed in the table and
    /// method binder scopes. Receiver arguments are captured when boxing;
    /// method arguments are supplied by each call.
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    pub arguments: Vec<Ty<I>>,
}

/// A checked executable entry owned by the module carrying this target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallableTarget {
    Script(FunctionRef),
    Native(NativeImportId),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BytecodeDebugMetadata {
    pub source_uri: Option<String>,
    pub source_module: Option<ModuleRef>,
    pub function_span: Span,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub source_spans: InstructionSourceSpanBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub line_table: LineTableBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub safe_debug_points: SafeDebugPointBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub local_live_ranges: LocalLiveRangeBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
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
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub params: TypeLayoutBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub locals: TypeLayoutBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub registers: TypeLayoutBuffer,
}

pub type BytecodeFunctionBuffer<I = DefinitionPath> = Vec<BytecodeFunction<I>>;

pub type BytecodeInstructionBuffer<I = DefinitionPath> = Vec<BytecodeInstruction<I>>;

pub type BytecodeModuleSlotBuffer = Vec<BytecodeModuleSlot>;

pub type ConstantPool = Vec<ConstantOperand>;

pub type BytecodeTypeTable = Vec<ValueType>;

pub type PathTable = Vec<PathRecord>;

pub type FunctionTable<I = DefinitionPath> = Vec<FunctionRecord<I>>;

pub type PublicItemRecord<I = DefinitionPath> = PublicItem<I>;

pub type PublicItemTable<I = DefinitionPath> = Vec<PublicItem<I>>;

pub type TypeLayoutBuffer = Vec<ValueType>;

pub type ControlFlowTargetBuffer = Vec<JumpTarget>;

pub type InstructionSourceSpanBuffer = Vec<InstructionSourceSpan>;

pub type LineTableBuffer = Vec<LineTableEntry>;

pub type SafeDebugPointBuffer = Vec<SafeDebugPoint>;

pub type LocalLiveRangeBuffer = Vec<LocalLiveRange>;

pub type CapturedBindingDebugBuffer = Vec<CapturedBindingDebugInfo>;

impl<I> Default for BytecodeModule<I> {
    fn default() -> Self {
        Self {
            dependencies: Default::default(),
            host_interface: Default::default(),
            native_imports: Default::default(),
            native_declarations: Default::default(),
            identity: Default::default(),
            source_name: Default::default(),
            module_slots: Default::default(),
            constants: Default::default(),
            types: Default::default(),
            structures: Default::default(),
            enumerations: Default::default(),
            interface_tables: Default::default(),
            paths: Default::default(),
            function_table: Default::default(),
            public_items: Default::default(),
            trait_contracts: Default::default(),
            functions: Default::default(),
        }
    }
}

impl<I> Default for BytecodeFunction<I> {
    fn default() -> Self {
        Self {
            id: Default::default(),
            identity: Default::default(),
            name: Default::default(),
            parameter_count: Default::default(),
            register_count: Default::default(),
            local_count: Default::default(),
            metadata: Default::default(),
            instructions: Default::default(),
        }
    }
}

impl<I> Default for FunctionMetadata<I> {
    fn default() -> Self {
        Self {
            semantic: Default::default(),
            params: Default::default(),
            return_type: Default::default(),
            locals: Default::default(),
            registers: Default::default(),
            roots: Default::default(),
            control_flow_targets: Default::default(),
            effects: Default::default(),
            debug: Default::default(),
        }
    }
}

mod mapping;
