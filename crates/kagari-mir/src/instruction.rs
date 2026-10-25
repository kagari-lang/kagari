use crate::ids::{BlockId, InstanceId, LocalId, ModuleSlotId, TempId};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    callable::{
        construction::evidence::{ConstructionRequirement, ConstructionWitness},
        interface::InterfaceCallContract,
        shared::SharedCall,
        witness::OperationWitness,
    },
    effects::{EffectSet, runtime_primitive_effects},
    native_import::NativeImport,
    numeric::{NumericConversion, NumericOperation},
    operations::{BinaryOp, IterOp, UnaryOp},
    standard::RuntimePrimitive,
};
use kagari_types::{
    declaration::path::HostPathDeclaration,
    reference::ReferenceAccess,
    ty::{NominalTy, Ty},
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

mod operands;
mod rewrite;

/// An exact selected table application or a capability supplied by a checked bound.
/// Bound targets retain the source receiver and complete interface application;
/// execution resolves them only in the current generic construction environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstructionTarget {
    Declared {
        implementation: DefinitionId,
        arguments: Vec<Ty>,
        operations: Vec<OperationWitness>,
        constructions: Vec<ConstructionWitness>,
    },
    Bound(ConstructionRequirement),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirValue {
    pub temp: TempId,
    pub ty: ValueType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AggregateFieldRef {
    pub owner: NominalTy,
    pub slot: usize,
    pub reference: Option<ReferenceAccess>,
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
    Await {
        dst: MirValue,
        value: MirValue,
        future: Ty,
    },
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
        ty: Ty,
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
        callee: CallTarget,
        args: ValueBuffer,
    },
    MakeTuple {
        dst: MirValue,
        elements: ValueBuffer,
    },
    RangeBound {
        dst: MirValue,
        value: MirValue,
        range: Ty,
        bound: Ty,
        upper: bool,
    },
    MakeRange {
        dst: MirValue,
        start: Option<MirValue>,
        end: Option<MirValue>,
        ty: Ty,
    },
    RepeatArray {
        element: Ty,
        dst: MirValue,
        value: MirValue,
        count: MirValue,
    },
    MakeArray {
        element: Ty,
        dst: MirValue,
        elements: ValueBuffer,
    },
    /// Capture every resume parameter without entering its body.
    MakeFuture {
        dst: MirValue,
        function: InstanceId,
        arguments: ValueBuffer,
        future: Ty,
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
        source: NominalTy,
        target: NominalTy,
    },
    MakeInterface {
        dst: MirValue,
        value: MirValue,
        target: ConstructionTarget,
    },
    MakeStruct {
        dst: MirValue,
        structure: NominalTy,
        fields: StructFieldInitBuffer,
    },
    MakeEnum {
        dst: MirValue,
        enumeration: NominalTy,
        variant: usize,
        fields: ValueBuffer,
    },
    TestEnumVariant {
        dst: MirValue,
        value: MirValue,
        enumeration: NominalTy,
        variant: usize,
    },
    ReadEnumPayload {
        dst: MirValue,
        value: MirValue,
        enumeration: NominalTy,
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

#[derive(Debug, Clone)]
pub enum CallTarget {
    Shared(Box<SharedCall>),
    SourceFunction(Box<SourceFunctionContract>),
    Function(InstanceId),
    InterfaceMethod(Box<InterfaceCallContract>),
    Native(Box<NativeImport>),
    Value(MirValue),
    Closure {
        value: MirValue,
        params: Vec<ValueType>,
        return_type: ValueType,
    },
    RuntimePrimitive(RuntimePrimitive),
    RuntimeHelper(RuntimeHelper),
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
            Self::Await { .. } => EffectSet {
                may_suspend: true,
                ..EffectSet::native_call()
            },
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
            Self::MakeRange { .. } => EffectSet {
                may_trap: true,
                ..EffectSet::default()
            },
            Self::MakeTuple { .. }
            | Self::MakeArray { .. }
            | Self::RepeatArray { .. }
            | Self::RangeBound { .. }
            | Self::MakeFuture { .. }
            | Self::MakeClosure { .. }
            | Self::MakeCell { .. }
            | Self::MakeInterface { .. }
            | Self::UpcastInterface { .. }
            | Self::MakeStruct { .. }
            | Self::MakeEnum { .. } => EffectSet::allocation(),

            Self::Iter { .. } => EffectSet::allocation().union(EffectSet::aggregate_write()),

            Self::ReadAggregateField { field, .. } | Self::WriteAggregateField { field, .. }
                if field.reference.is_some() =>
            {
                EffectSet::native_call()
            }
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
    Char(char),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub arguments: Vec<Ty>,
    pub params: Vec<ValueType>,
    pub return_type: ValueType,
}

mod mapping;
