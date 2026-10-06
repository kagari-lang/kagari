use crate::ids::{BlockId, InstanceId, LocalId, ModuleSlotId, TempId};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionPath, reference::DefinitionReference};
use kagari_contract::{
    callable::{interface::InterfaceCallContract, shared::SharedCall},
    effects::{EffectSet, runtime_primitive_effects},
    native_import::NativeImport,
    numeric::{NumericConversion, NumericOperation},
    operations::{BinaryOp, IterOp, UnaryOp},
    standard::RuntimePrimitive,
};
use kagari_types::{
    host_interface::path::HostPathDeclaration,
    ty::{NominalTy, Ty},
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

mod operands;
mod rewrite;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirValue {
    pub temp: TempId,
    pub ty: ValueType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct AggregateFieldRef<I = DefinitionPath> {
    pub owner: NominalTy<I>,
    pub slot: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct PathRef<I = DefinitionPath> {
    pub declaration: Option<HostPathDeclaration<I>>,
    pub contract_fingerprint: u64,
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
pub enum Instruction<I = DefinitionPath> {
    Convert {
        dst: MirValue,
        src: MirValue,
        conversion: NumericConversion,
    },
    Numeric {
        dst: MirValue,
        operation: NumericOperation,
        lhs: MirValue,
        rhs: Option<MirValue>,
    },
    Iter {
        dst: MirValue,
        value: Option<MirValue>,
        ty: Ty<I>,
        op: IterOp,
    },
    LoadConst {
        dst: MirValue,
        constant: Constant,
    },
    LoadLocal {
        dst: MirValue,
        local: LocalId,
    },
    LoadModule {
        dst: MirValue,
        slot: ModuleSlotId,
    },
    StoreLocal {
        local: LocalId,
        src: MirValue,
    },
    StoreModule {
        slot: ModuleSlotId,
        src: MirValue,
    },
    Move {
        dst: MirValue,
        src: MirValue,
    },
    Unary {
        dst: MirValue,
        op: UnaryOp,
        operand: MirValue,
    },
    Binary {
        dst: MirValue,
        op: BinaryOp,
        lhs: MirValue,
        rhs: MirValue,
    },
    Call {
        dst: Option<MirValue>,
        callee: CallTarget<I>,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        args: ValueBuffer,
    },
    BeginIteration {
        collection: MirValue,
    },
    EndIteration,
    MakeTuple {
        dst: MirValue,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        elements: ValueBuffer,
    },
    RangeBound {
        dst: MirValue,
        value: MirValue,
        range: Ty<I>,
        bound: Ty<I>,
        upper: bool,
    },
    MakeRange {
        dst: MirValue,
        start: Option<MirValue>,
        end: Option<MirValue>,
        ty: Ty<I>,
    },
    RepeatArray {
        element: Ty<I>,
        dst: MirValue,
        value: MirValue,
        count: MirValue,
    },
    MakeArray {
        element: Ty<I>,
        dst: MirValue,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        elements: ValueBuffer,
    },
    MakeClosure {
        dst: MirValue,
        function: InstanceId,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        captures: ValueBuffer,
    },
    MakeCell {
        dst: MirValue,
        value: MirValue,
    },
    ReadCell {
        dst: MirValue,
        cell: MirValue,
    },
    WriteCell {
        cell: MirValue,
        value: MirValue,
    },
    UpcastInterface {
        dst: MirValue,
        value: MirValue,
        source: NominalTy<I>,
        target: NominalTy<I>,
    },
    MakeInterface {
        dst: MirValue,
        value: MirValue,
        implementation: I,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        arguments: Vec<Ty<I>>,
    },
    MakeStruct {
        dst: MirValue,
        structure: NominalTy<I>,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        fields: StructFieldInitBuffer,
    },
    MakeEnum {
        dst: MirValue,
        enumeration: NominalTy<I>,
        variant: usize,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        fields: ValueBuffer,
    },
    TestEnumVariant {
        dst: MirValue,
        value: MirValue,
        enumeration: NominalTy<I>,
        variant: usize,
    },
    ReadEnumPayload {
        dst: MirValue,
        value: MirValue,
        enumeration: NominalTy<I>,
        variant: usize,
        index: usize,
    },
    ReadAggregateField {
        dst: MirValue,
        base: MirValue,
        field: AggregateFieldRef<I>,
    },
    WriteAggregateField {
        base: MirValue,
        field: AggregateFieldRef<I>,
        value: MirValue,
    },
    ReadAggregateIndex {
        dst: MirValue,
        base: MirValue,
        index: MirValue,
    },
    WriteAggregateIndex {
        base: MirValue,
        index: MirValue,
        value: MirValue,
    },
    ReadPath {
        dst: MirValue,
        root_or_view: MirValue,
        path: PathRef<I>,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        dynamic_args: ValueBuffer,
    },
    SetPath {
        root_or_view: MirValue,
        path: PathRef<I>,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        dynamic_args: ValueBuffer,
        value: MirValue,
    },
    ModifyPath {
        dst: Option<MirValue>,
        root_or_view: MirValue,
        path: PathRef<I>,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        dynamic_args: ValueBuffer,
        op: BinaryOp,
        value: MirValue,
    },
    MakePathView {
        dst: MirValue,
        root_or_view: MirValue,
        path: PathRef<I>,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        dynamic_args: ValueBuffer,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Terminator {
    Return(Option<MirValue>),
    Jump(BlockId),
    Branch {
        cond: MirValue,
        then_block: BlockId,
        else_block: BlockId,
    },
    Unreachable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub enum CallTarget<I = DefinitionPath> {
    Shared(Box<SharedCall<I>>),
    SourceFunction(Box<SourceFunctionContract<I>>),
    Function(InstanceId),
    InterfaceMethod(Box<InterfaceCallContract<I>>),
    Native(Box<NativeImport<I>>),
    Value(MirValue),
    Closure {
        value: MirValue,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        params: Vec<ValueType>,
        return_type: ValueType,
    },
    RuntimePrimitive(RuntimePrimitive),
    RuntimeHelper(RuntimeHelper),
}

impl<I: DefinitionReference> Instruction<I> {
    pub fn path_reference(&self) -> Option<&PathRef<I>> {
        match self {
            Self::ReadPath { path, .. }
            | Self::SetPath { path, .. }
            | Self::ModifyPath { path, .. }
            | Self::MakePathView { path, .. } => Some(path),
            _ => None,
        }
    }

    pub fn effects(&self) -> EffectSet {
        match self {
            Self::LoadConst { .. } | Self::Move { .. } => EffectSet::default(),
            Self::Convert { conversion, .. } => {
                let _ = conversion;
                // Runtime conversion validates the source numeric domain.
                EffectSet {
                    may_trap: true,
                    ..EffectSet::default()
                }
            }
            Self::Numeric { .. } => EffectSet {
                may_trap: true,
                ..EffectSet::default()
            },
            Self::Unary { op, operand, .. } => EffectSet {
                may_trap: matches!(op, UnaryOp::Neg)
                    && matches!(operand.ty, ValueType::I32 | ValueType::I64),
                ..EffectSet::default()
            },
            Self::Binary { op, lhs, .. } => {
                let heap_comparison = matches!(
                    op,
                    BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::IdentityEq | BinaryOp::IdentityNotEq
                ) && lhs.ty == ValueType::HeapObject;
                EffectSet {
                    reads_aggregate: heap_comparison,
                    may_trap: matches!(op, BinaryOp::Numeric(_))
                        || heap_comparison
                        || (matches!(
                            op,
                            BinaryOp::Add
                                | BinaryOp::Sub
                                | BinaryOp::Mul
                                | BinaryOp::Div
                                | BinaryOp::Rem
                        ) && matches!(
                            lhs.ty,
                            ValueType::I32 | ValueType::I64 | ValueType::U64
                        )),
                    ..EffectSet::default()
                }
            }
            Self::LoadLocal { .. } => EffectSet::local_read(),
            Self::StoreLocal { .. } => EffectSet::local_write(),
            Self::LoadModule { .. } => EffectSet::module_read(),
            Self::StoreModule { .. } => EffectSet::module_write(),
            Self::Call { callee, .. } => callee.effects(),
            Self::BeginIteration { .. } | Self::EndIteration => EffectSet::runtime_call(),
            Self::MakeRange { .. } => EffectSet {
                may_trap: true,
                ..EffectSet::default()
            },
            Self::MakeTuple { .. }
            | Self::MakeArray { .. }
            | Self::RepeatArray { .. }
            | Self::RangeBound { .. }
            | Self::MakeClosure { .. }
            | Self::MakeCell { .. }
            | Self::MakeInterface { .. }
            | Self::UpcastInterface { .. }
            | Self::MakeStruct { .. }
            | Self::MakeEnum { .. } => EffectSet::allocation(),

            Self::Iter { .. } => EffectSet::allocation().union(EffectSet::aggregate_write()),

            Self::ReadAggregateField { .. }
            | Self::ReadCell { .. }
            | Self::ReadAggregateIndex { .. }
            | Self::TestEnumVariant { .. }
            | Self::ReadEnumPayload { .. } => EffectSet::aggregate_read(),
            Self::WriteAggregateField { .. } | Self::WriteAggregateIndex { .. } => {
                EffectSet::aggregate_write()
            }
            Self::WriteCell { .. } => EffectSet::aggregate_write(),
            Self::ReadPath { .. } | Self::MakePathView { .. } => EffectSet::path_read(),
            Self::SetPath { .. } => EffectSet::path_write(),
            Self::ModifyPath { .. } => EffectSet::path_read().union(EffectSet::path_write()),
        }
    }
}

impl Terminator {
    pub fn effects(&self) -> EffectSet {
        EffectSet {
            may_trap: matches!(self, Self::Unreachable),
            ..EffectSet::default()
        }
    }
}

impl<I: DefinitionReference> CallTarget<I> {
    pub fn effects(&self) -> EffectSet {
        match self {
            Self::Function(_) | Self::SourceFunction(_) | Self::Value(_) | Self::Closure { .. } => {
                EffectSet::call()
            }
            Self::InterfaceMethod(_) | Self::Shared(_) => EffectSet::runtime_call(),
            Self::Native(_) => EffectSet::native_call(),
            Self::RuntimePrimitive(intrinsic) => runtime_primitive_effects(*intrinsic),
            Self::RuntimeHelper(helper) => helper.effects(),
        }
    }
}

impl RuntimeHelper {
    pub fn effects(&self) -> EffectSet {
        match self {
            Self::DynamicCall => EffectSet::runtime_call(),
            Self::ReflectTypeOf | Self::ReflectGetField(_) => {
                EffectSet::runtime_call().union(EffectSet::aggregate_read())
            }
            Self::ReflectSetField(_) | Self::ReflectSetIndex => {
                EffectSet::runtime_call().union(EffectSet::aggregate_write())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeHelper {
    ReflectTypeOf,
    ReflectGetField(String),
    ReflectSetField(String),
    ReflectSetIndex,
    DynamicCall,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Str(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructFieldInit {
    pub slot: usize,
    pub value: MirValue,
}

pub type InstructionBuffer<I = DefinitionPath> = Vec<Instruction<I>>;

pub type ValueBuffer = SmallVec<[MirValue; 4]>;

pub type StructFieldInitBuffer = SmallVec<[StructFieldInit; 4]>;

/// Unlinked declaration contract. It cannot be encoded as an executable call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct SourceFunctionContract<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    pub arguments: Vec<Ty<I>>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    pub params: Vec<ValueType>,
    pub return_type: ValueType,
}

mod mapping;
