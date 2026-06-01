use kagari_hir::{
    builtin::{BuiltinMethod, surface::StandardIntrinsic},
    hir,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::module::ids::{BlockId, LocalId, ModuleSlotId, TempId};
use crate::module::types::ValueType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrValue {
    pub temp: TempId,
    pub ty: ValueType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AggregateFieldRef {
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRef {
    pub root_ty: ValueType,
    pub result_ty: ValueType,
    pub read_only: bool,
    pub debug_name: String,
}

#[derive(Debug, Clone)]
pub enum Instruction {
    LoadConst {
        dst: IrValue,
        constant: Constant,
    },
    LoadLocal {
        dst: IrValue,
        local: LocalId,
    },
    LoadModule {
        dst: IrValue,
        slot: ModuleSlotId,
    },
    StoreLocal {
        local: LocalId,
        src: IrValue,
    },
    StoreModule {
        slot: ModuleSlotId,
        src: IrValue,
    },
    Move {
        dst: IrValue,
        src: IrValue,
    },
    Unary {
        dst: IrValue,
        op: UnaryOp,
        operand: IrValue,
    },
    Binary {
        dst: IrValue,
        op: BinaryOp,
        lhs: IrValue,
        rhs: IrValue,
    },
    Call {
        dst: Option<IrValue>,
        callee: CallTarget,
        args: ValueBuffer,
    },
    MakeTuple {
        dst: IrValue,
        elements: ValueBuffer,
    },
    MakeArray {
        dst: IrValue,
        elements: ValueBuffer,
    },
    MakeStruct {
        dst: IrValue,
        name: String,
        fields: StructFieldInitBuffer,
    },
    ReadAggregateField {
        dst: IrValue,
        base: IrValue,
        field: AggregateFieldRef,
    },
    WriteAggregateField {
        base: IrValue,
        field: AggregateFieldRef,
        value: IrValue,
    },
    ReadAggregateIndex {
        dst: IrValue,
        base: IrValue,
        index: IrValue,
    },
    WriteAggregateIndex {
        base: IrValue,
        index: IrValue,
        value: IrValue,
    },
    ReadPath {
        dst: IrValue,
        root_or_view: IrValue,
        path: PathRef,
        dynamic_args: ValueBuffer,
    },
    SetPath {
        root_or_view: IrValue,
        path: PathRef,
        dynamic_args: ValueBuffer,
        value: IrValue,
    },
    ModifyPath {
        dst: Option<IrValue>,
        root_or_view: IrValue,
        path: PathRef,
        dynamic_args: ValueBuffer,
        op: BinaryOp,
        value: IrValue,
    },
    MakePathView {
        dst: IrValue,
        root_or_view: IrValue,
        path: PathRef,
        dynamic_args: ValueBuffer,
    },
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Return(Option<IrValue>),
    Jump(BlockId),
    Branch {
        cond: IrValue,
        then_block: BlockId,
        else_block: BlockId,
    },
    Unreachable,
}

#[derive(Debug, Clone)]
pub enum CallTarget {
    Function(hir::FunctionId),
    Value(IrValue),
    BuiltinMethod(BuiltinMethod),
    StandardIntrinsic(StandardIntrinsic),
    RuntimeHelper(RuntimeHelper),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSet {
    pub reads_local: bool,
    pub writes_local: bool,
    pub reads_module: bool,
    pub writes_module: bool,
    pub reads_aggregate: bool,
    pub writes_aggregate: bool,
    pub reads_path: bool,
    pub writes_path: bool,
    pub allocates: bool,
    pub calls: bool,
    pub touches_runtime: bool,
    pub may_trap: bool,
}

impl EffectSet {
    pub fn union(self, other: Self) -> Self {
        Self {
            reads_local: self.reads_local || other.reads_local,
            writes_local: self.writes_local || other.writes_local,
            reads_module: self.reads_module || other.reads_module,
            writes_module: self.writes_module || other.writes_module,
            reads_aggregate: self.reads_aggregate || other.reads_aggregate,
            writes_aggregate: self.writes_aggregate || other.writes_aggregate,
            reads_path: self.reads_path || other.reads_path,
            writes_path: self.writes_path || other.writes_path,
            allocates: self.allocates || other.allocates,
            calls: self.calls || other.calls,
            touches_runtime: self.touches_runtime || other.touches_runtime,
            may_trap: self.may_trap || other.may_trap,
        }
    }

    pub fn local_read() -> Self {
        Self {
            reads_local: true,
            ..Self::default()
        }
    }

    pub fn local_write() -> Self {
        Self {
            writes_local: true,
            ..Self::default()
        }
    }

    pub fn aggregate_read() -> Self {
        Self {
            reads_aggregate: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn module_read() -> Self {
        Self {
            reads_module: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn module_write() -> Self {
        Self {
            writes_module: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn aggregate_write() -> Self {
        Self {
            writes_aggregate: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn path_read() -> Self {
        Self {
            reads_path: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn path_write() -> Self {
        Self {
            writes_path: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn allocation() -> Self {
        Self {
            allocates: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn call() -> Self {
        Self {
            calls: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn runtime_call() -> Self {
        Self {
            calls: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }
}

impl Instruction {
    pub fn effects(&self) -> EffectSet {
        match self {
            Self::LoadConst { .. }
            | Self::Move { .. }
            | Self::Unary { .. }
            | Self::Binary { .. } => EffectSet::default(),
            Self::LoadLocal { .. } => EffectSet::local_read(),
            Self::StoreLocal { .. } => EffectSet::local_write(),
            Self::LoadModule { .. } => EffectSet::module_read(),
            Self::StoreModule { .. } => EffectSet::module_write(),
            Self::Call { callee, .. } => callee.effects(),
            Self::MakeTuple { .. } | Self::MakeArray { .. } | Self::MakeStruct { .. } => {
                EffectSet::allocation()
            }
            Self::ReadAggregateField { .. } | Self::ReadAggregateIndex { .. } => {
                EffectSet::aggregate_read()
            }
            Self::WriteAggregateField { .. } | Self::WriteAggregateIndex { .. } => {
                EffectSet::aggregate_write()
            }
            Self::ReadPath { .. } | Self::MakePathView { .. } => EffectSet::path_read(),
            Self::SetPath { .. } => EffectSet::path_write(),
            Self::ModifyPath { .. } => EffectSet::path_read().union(EffectSet::path_write()),
        }
    }
}

impl Terminator {
    pub fn effects(&self) -> EffectSet {
        EffectSet::default()
    }
}

impl CallTarget {
    pub fn effects(&self) -> EffectSet {
        match self {
            Self::Function(_) | Self::Value(_) => EffectSet::call(),
            Self::BuiltinMethod(method) => match method {
                BuiltinMethod::Array(kagari_hir::builtin::array::Method::Push)
                | BuiltinMethod::Array(kagari_hir::builtin::array::Method::Pop) => {
                    EffectSet::runtime_call().union(EffectSet::aggregate_write())
                }
                BuiltinMethod::Array(kagari_hir::builtin::array::Method::Len)
                | BuiltinMethod::Iterable(_)
                | BuiltinMethod::String(kagari_hir::builtin::StringMethod::Len) => {
                    EffectSet::runtime_call().union(EffectSet::aggregate_read())
                }
            },
            Self::StandardIntrinsic(intrinsic) => standard_intrinsic_effects(*intrinsic),
            Self::RuntimeHelper(helper) => helper.effects(),
        }
    }
}

fn standard_intrinsic_effects(intrinsic: StandardIntrinsic) -> EffectSet {
    use StandardIntrinsic::*;

    let runtime_read = EffectSet::runtime_call().union(EffectSet::aggregate_read());
    let mutating = matches!(
        intrinsic,
        ArrayPush
            | ArrayPop
            | ArrayInsert
            | ArrayRemove
            | ArrayClear
            | MapInsert
            | MapRemove
            | MapClear
            | SetInsert
            | SetRemove
            | SetClear
    );
    let allocating = matches!(
        intrinsic,
        ArrayGet
            | ArrayPop
            | ArrayRemove
            | MapNew
            | MapGet
            | MapRemove
            | MapKeys
            | MapValues
            | MapEntries
            | SetNew
            | SetToArray
            | SetUnion
            | SetIntersection
            | SetDifference
            | StringSlice
            | OptionMap
            | OptionAndThen
            | ResultMap
            | ResultMapErr
            | ResultAndThen
            | IterGet
            | IterToArray
    );

    let mut effects = match intrinsic {
        MathMin | MathMax | MathClamp | MathAbs | MathFloor | MathCeil | MathRound | MathSqrt
        | MathSin | MathCos | MathTan => EffectSet::runtime_call(),
        DebugPrint | DebugAssert | DebugAssertEq | DebugPanic => EffectSet::runtime_call(),
        _ => runtime_read,
    };
    if mutating {
        effects = effects.union(EffectSet::aggregate_write());
    }
    if allocating {
        effects = effects.union(EffectSet::allocation());
    }
    effects
}

impl RuntimeHelper {
    pub fn effects(&self) -> EffectSet {
        match self {
            Self::HostFunction(_) | Self::DynamicCall => EffectSet::runtime_call(),
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
    HostFunction(String),
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
    F32(f32),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
}

#[derive(Debug, Clone)]
pub struct StructFieldInit {
    pub name: String,
    pub value: IrValue,
}

pub type InstructionBuffer = Vec<Instruction>;
pub type ValueBuffer = SmallVec<[IrValue; 4]>;
pub type StructFieldInitBuffer = SmallVec<[StructFieldInit; 4]>;
