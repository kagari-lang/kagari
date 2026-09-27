use kagari_abi::effects::{EffectSet, standard_intrinsic_effects};
use kagari_abi::numeric::NumericConversion;
use kagari_abi::numeric::NumericOperation;
use kagari_abi::operations::{BinaryOp, IterOp, StandardEnumOp, UnaryOp};
use kagari_abi::standard::StandardIntrinsic;
use kagari_abi::types::AbiType;
use kagari_abi::types::NominalAbiType;
use kagari_common::host_interface::HostFunctionDeclaration;
use kagari_common::host_interface::HostPathDeclaration;
use kagari_common::identity::DefinitionId;
use smallvec::SmallVec;

use crate::ids::BlockId;
use crate::ids::InstanceId;
use crate::ids::LocalId;
use crate::ids::ModuleSlotId;
use crate::ids::TempId;
use kagari_abi::representation::ValueType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirValue {
    pub temp: TempId,
    pub ty: ValueType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AggregateFieldRef {
    pub owner: NominalAbiType,
    pub slot: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRef {
    pub declaration: Option<HostPathDeclaration>,
    pub contract_fingerprint: u64,
    pub root_ty: ValueType,
    pub result_ty: ValueType,
    pub read_only: bool,
    pub debug_name: String,
}

#[derive(Debug, Clone)]
pub enum Instruction {
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
    MapResultError {
        dst: MirValue,
        original: MirValue,
        error: MirValue,
        ty: AbiType,
    },
    Iter {
        dst: MirValue,
        value: Option<MirValue>,
        ty: AbiType,
        op: IterOp,
    },
    StandardEnum {
        dst: MirValue,
        value: Option<MirValue>,
        ty: AbiType,
        op: StandardEnumOp,
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
        callee: CallTarget,
        args: ValueBuffer,
    },
    BeginIteration {
        collection: MirValue,
    },
    EndIteration,
    MakeTuple {
        dst: MirValue,
        elements: ValueBuffer,
    },
    RangeBound {
        dst: MirValue,
        value: MirValue,
        range: AbiType,
        bound: AbiType,
        upper: bool,
    },
    MakeRange {
        dst: MirValue,
        start: Option<MirValue>,
        end: Option<MirValue>,
        ty: AbiType,
    },
    RepeatArray {
        dst: MirValue,
        value: MirValue,
        count: MirValue,
    },
    MakeArray {
        dst: MirValue,
        elements: ValueBuffer,
    },
    MakeClosure {
        dst: MirValue,
        function: InstanceId,
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
        source: NominalAbiType,
        target: NominalAbiType,
    },
    MakeInterface {
        dst: MirValue,
        value: MirValue,
        implementation: DefinitionId,
        arguments: Vec<AbiType>,
    },
    MakeStruct {
        dst: MirValue,
        structure: NominalAbiType,
        fields: StructFieldInitBuffer,
    },
    MakeEnum {
        dst: MirValue,
        enumeration: NominalAbiType,
        variant: usize,
        fields: ValueBuffer,
    },
    TestEnumVariant {
        dst: MirValue,
        value: MirValue,
        enumeration: NominalAbiType,
        variant: usize,
    },
    ReadEnumPayload {
        dst: MirValue,
        value: MirValue,
        enumeration: NominalAbiType,
        variant: usize,
        index: usize,
    },
    ReadAggregateField {
        dst: MirValue,
        base: MirValue,
        field: AggregateFieldRef,
    },
    WriteAggregateField {
        base: MirValue,
        field: AggregateFieldRef,
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
        path: PathRef,
        dynamic_args: ValueBuffer,
    },
    SetPath {
        root_or_view: MirValue,
        path: PathRef,
        dynamic_args: ValueBuffer,
        value: MirValue,
    },
    ModifyPath {
        dst: Option<MirValue>,
        root_or_view: MirValue,
        path: PathRef,
        dynamic_args: ValueBuffer,
        op: BinaryOp,
        value: MirValue,
    },
    MakePathView {
        dst: MirValue,
        root_or_view: MirValue,
        path: PathRef,
        dynamic_args: ValueBuffer,
    },
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum CallTarget {
    SourceFunction(Box<SourceFunctionContract>),
    Function(InstanceId),
    InterfaceMethod(Box<InterfaceCallContract>),
    HostFunction(Box<HostFunctionDeclaration>),
    Value(MirValue),
    Closure {
        value: MirValue,
        params: Vec<ValueType>,
        return_type: ValueType,
    },
    StandardIntrinsic(StandardIntrinsic),
    RuntimeHelper(RuntimeHelper),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCallContract {
    pub interface: NominalAbiType,
    pub method_slot: u32,
}

impl Instruction {
    pub fn path_reference(&self) -> Option<&PathRef> {
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
                if conversion.checked {
                    EffectSet::allocation()
                } else {
                    EffectSet::default()
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
                        ) && matches!(lhs.ty, ValueType::I32 | ValueType::I64)),
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
            Self::StandardEnum {
                op: StandardEnumOp::Make(_),
                ..
            } => EffectSet::allocation(),
            Self::MapResultError { .. } => EffectSet::allocation(),
            Self::Iter { .. } => EffectSet::allocation().union(EffectSet::aggregate_write()),
            Self::StandardEnum { .. } => EffectSet::aggregate_read(),
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

impl CallTarget {
    pub fn effects(&self) -> EffectSet {
        match self {
            Self::Function(_) | Self::SourceFunction(_) | Self::Value(_) | Self::Closure { .. } => {
                EffectSet::call()
            }
            Self::InterfaceMethod(_) => EffectSet::runtime_call(),
            Self::HostFunction(declaration) => EffectSet {
                allocates: declaration.effects.may_allocate,
                ..EffectSet::runtime_call()
            },
            Self::StandardIntrinsic(intrinsic) => standard_intrinsic_effects(*intrinsic),
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

#[derive(Debug, Clone)]
pub enum RuntimeHelper {
    ReflectTypeOf,
    ReflectGetField(String),
    ReflectSetField(String),
    ReflectSetIndex,
    DynamicCall,
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone)]
pub struct StructFieldInit {
    pub slot: usize,
    pub value: MirValue,
}

pub type InstructionBuffer = Vec<Instruction>;
pub type ValueBuffer = SmallVec<[MirValue; 4]>;
pub type StructFieldInitBuffer = SmallVec<[StructFieldInit; 4]>;

/// Unlinked declaration contract. It cannot be encoded as an executable call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFunctionContract {
    pub declaration: DefinitionId,
    pub arguments: Vec<AbiType>,
    pub params: Vec<ValueType>,
    pub return_type: ValueType,
}
