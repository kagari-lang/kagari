use crate::{
    instruction::{BytecodeInstruction, ConstantOperand, JumpTarget, NativeImportId, PathId, Slot},
    program::ModuleRef,
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
    span::Span,
};
use kagari_contract::{
    callable::generic::GenericBody,
    effects::EffectSet,
    ids::{DebugPointId, FunctionRef},
    layout::{EnumLayout, StructLayout},
    native_import::NativeImport,
    representation::semantic_representation,
    slots::{self as semantic_slots, FunctionTypeFacts},
    types::{ConcreteFunctionIdentity, PublicItem, TraitContract},
};
use kagari_types::{
    declaration::{NativeDeclaration, TypeDeclaration},
    declaration::{path::HostPathDeclaration, requirement::NativeCallableRequirement},
    ty::{NominalTy, Ty},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, iter};

#[derive(Debug, Clone, Default)]
pub struct BytecodeModule {
    pub dependencies: Vec<ModuleRef>,
    pub external_types: Vec<TypeDeclaration>,
    /// Explicit portable path requirements over the shared external schemas.
    pub path_declarations: Vec<HostPathDeclaration>,
    pub native_imports: Vec<NativeImport>,
    pub native_declarations: Vec<NativeDeclaration>,
    pub identity: ModuleIdentity,
    pub source_name: String,
    pub module_slots: BytecodeModuleSlotBuffer,
    pub constants: ConstantPool,
    pub types: BytecodeTypeTable,
    pub structures: Vec<StructLayout>,
    pub enumerations: Vec<EnumLayout>,
    pub interface_tables: Vec<InterfaceTableRecord>,
    pub paths: PathTable,
    pub function_table: FunctionTable,
    pub public_items: PublicItemTable,
    /// Exact foreign source requirements; these do not define or export a trait.
    pub trait_references: Vec<TraitContract>,
    pub trait_contracts: Vec<TraitContract>,
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

#[derive(Debug, Clone, Default)]
pub struct BytecodeFunction {
    pub id: FunctionRef,
    pub identity: Option<ConcreteFunctionIdentity>,
    pub name: String,
    pub parameter_count: u16,
    pub slot_count: u32,
    pub metadata: FunctionMetadata,
    pub instructions: BytecodeInstructionBuffer,
}

#[derive(Debug, Clone, Default)]
pub struct FunctionMetadata {
    pub semantic: BytecodeSemanticSlots,
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    pub slots: TypeLayoutBuffer,
    pub parameter_slots: Vec<Slot>,
    pub origins: Vec<InstructionOrigin>,
    /// Actual instruction origins for PCs with multiple semantic origins, ordered by PC.
    /// Every ambiguous PC requires exactly one entry; a unique origin needs no override.
    pub instruction_origins: Vec<usize>,
    pub roots: RootSlotLayout,
    pub control_flow_targets: ControlFlowTargetBuffer,
    pub effects: EffectSet,
    pub debug: BytecodeDebugMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionRecord {
    pub id: FunctionRef,
    pub identity: Option<ConcreteFunctionIdentity>,
    pub name: String,
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    pub effects: EffectSet,
}

/// Conservative logical roots derived from the complete executable slot layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootSlotLayout {
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub slots: Vec<Slot>,
}

impl RootSlotLayout {
    pub fn from_types(slots: &[ValueType]) -> Self {
        Self {
            slots: slots
                .iter()
                .enumerate()
                .filter(|(_, ty)| ty.may_contain_gc_reference())
                .map(|(index, _)| Slot::new(index))
                .collect(),
        }
    }
}

/// A semantic MIR origin maps to a real executable PC; its span is not strippable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionOrigin {
    pub instruction: usize,
    pub span: Span,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// Full callable and slot facts projected from MIR, independent of MIR namespaces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BytecodeSemanticSlots {
    pub generic: Option<GenericBody>,
    pub protocol_adapter: Option<NativeCallableRequirement>,
    pub params: BTreeMap<usize, Ty>,
    pub result: Option<Ty>,
    pub slots: BTreeMap<usize, Ty>,
}

impl BytecodeSemanticSlots {
    pub fn types_valid(
        &self,
        identity: Option<&ConcreteFunctionIdentity>,
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> bool {
        semantic_slots::types_valid(self, identity, cancel, definitions)
    }

    pub fn protocol_adapter_valid(
        &self,
        identity: Option<&ConcreteFunctionIdentity>,
        definitions: &DefinitionTable,
    ) -> bool {
        semantic_slots::protocol_adapter_valid(
            self.protocol_adapter.as_ref(),
            self.generic.as_ref(),
            &self.params,
            self.result.as_ref(),
            identity,
            definitions,
        )
    }

    pub fn generic_layout_valid(
        &self,
        slots: &[ValueType],
        parameter_slots: &[Slot],
        result: ValueType,
    ) -> bool {
        if parameter_slots
            .iter()
            .any(|slot| slot.index() >= slots.len())
        {
            return false;
        }
        slots
            .iter()
            .enumerate()
            .map(|(index, &repr)| (repr, self.slots.get(&index)))
            .chain(
                parameter_slots
                    .iter()
                    .enumerate()
                    .map(|(index, slot)| (slots[slot.index()], self.params.get(&index))),
            )
            .chain(iter::once((result, self.result.as_ref())))
            .all(|(repr, ty)| {
                let generic =
                    ty.is_some_and(|ty| semantic_representation(ty) == ValueType::Generic);
                if repr == ValueType::Generic || generic {
                    self.generic.is_some() && generic && repr == ValueType::Generic
                } else {
                    true
                }
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceTableRecord {
    /// Optional dynamic surface that hides a concrete associated iterator.
    pub view: Option<InterfaceViewRecord>,
    /// Ordered impl arguments. An empty record for a generic template retains
    /// static method instances and cannot be selected by MakeInterface.
    pub arguments: Vec<Ty>,
    pub declaration: DefinitionId,
    pub methods: Vec<InterfaceMethodSlot>,
    /// Preselected ancestor tables, including their receiver argument mappings.
    pub parents: Vec<InterfaceParentRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceParentRecord {
    pub interface: NominalTy,
    pub implementation: ConcreteFunctionIdentity,
    pub view: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceViewRecord {
    pub interface: NominalTy,
    /// Only changed return representations have an adapter; raw slots remain exact.
    pub results: Vec<InterfaceResultAdapter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceResultAdapter {
    pub method: DefinitionId,
    pub implementation: ConcreteFunctionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceMethodSlot {
    pub method: DefinitionId,
    pub target: CallableTarget,
    /// Arguments for the target's shared entry, expressed in the table and
    /// method binder scopes. Receiver arguments are captured when boxing;
    /// method arguments are supplied by each call.
    pub arguments: Vec<Ty>,
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
    pub slot: Slot,
    pub binding: u32,
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
    pub slots: TypeLayoutBuffer,
}

pub type BytecodeFunctionBuffer = Vec<BytecodeFunction>;

pub type BytecodeInstructionBuffer = Vec<BytecodeInstruction>;

pub type BytecodeModuleSlotBuffer = Vec<BytecodeModuleSlot>;

pub type ConstantPool = Vec<ConstantOperand>;

pub type BytecodeTypeTable = Vec<ValueType>;

pub type PathTable = Vec<PathRecord>;

pub type FunctionTable = Vec<FunctionRecord>;

pub type PublicItemRecord = PublicItem;

pub type PublicItemTable = Vec<PublicItem>;

pub type TypeLayoutBuffer = Vec<ValueType>;

pub type ControlFlowTargetBuffer = Vec<JumpTarget>;

pub type InstructionSourceSpanBuffer = Vec<InstructionSourceSpan>;

pub type LineTableBuffer = Vec<LineTableEntry>;

pub type SafeDebugPointBuffer = Vec<SafeDebugPoint>;

pub type LocalLiveRangeBuffer = Vec<LocalLiveRange>;

pub type CapturedBindingDebugBuffer = Vec<CapturedBindingDebugInfo>;

mod mapping;
mod origins;
mod paths;

pub(crate) mod slots;

impl FunctionTypeFacts for BytecodeSemanticSlots {
    fn generic(&self) -> Option<&GenericBody> {
        self.generic.as_ref()
    }

    fn protocol_adapter(&self) -> Option<&NativeCallableRequirement> {
        self.protocol_adapter.as_ref()
    }

    fn params(&self) -> &BTreeMap<usize, Ty> {
        &self.params
    }

    fn result(&self) -> Option<&Ty> {
        self.result.as_ref()
    }

    fn value_types(&self) -> impl Iterator<Item = &Ty> {
        self.slots.values()
    }
}
