use crate::{module::CallableTarget, program::ModuleRef};
use kagari_abi::representation::ValueType;
use kagari_contract::{
    callable::{
        construction::evidence::ConstructionRequirement, interface::InterfaceCallContract,
        shared::SharedCall,
    },
    ids::FunctionRef,
    numeric::{NumericConversion, NumericOperation},
    operations::IterOp,
    standard::RuntimePrimitive,
};
use kagari_types::{
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use serde::{Deserialize, Serialize};
use std::{
    hash::{Hash, Hasher},
    mem,
};

/// An exact selected table application or a capability supplied by a checked bound.
/// Bound targets retain the source receiver and complete interface application;
/// execution resolves them only in the current generic construction environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstructionTarget {
    Declared {
        module: ModuleRef,
        implementation: InterfaceTableRef,
        arguments: Vec<Ty>,
    },
    Bound(ConstructionRequirement),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Slot(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnumId(u32);

impl EnumId {
    pub fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("enum slot overflow"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InterfaceTableRef(u32);

impl InterfaceTableRef {
    pub fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("interface table slot overflow"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl Slot {
    pub fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("slot ID overflow"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleSlot(u16);

impl ModuleSlot {
    pub fn new(index: usize) -> Self {
        Self(index as u16)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JumpTarget(u32);

impl JumpTarget {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StructId(u32);

impl StructId {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldRef {
    pub structure: StructId,
    pub arguments: Vec<Ty>,
    pub slot: u32,
    pub reference: Option<ReferenceAccess>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PathId(u32);

impl PathId {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index of a concrete engine binding resolved when the module is linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeImportId(u32);

impl NativeImportId {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// An index into the owning module's portable constant table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConstantId(u32);

impl ConstantId {
    pub fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("constant table index overflow"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A numeric constant retains its full source width, not just its physical carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalarOperand {
    Slot(Slot),
    Constant {
        constant: ConstantId,
        ty: BuiltinType,
    },
}

impl ScalarOperand {
    pub fn slot(self) -> Option<Slot> {
        match self {
            Self::Slot(slot) => Some(slot),
            Self::Constant { .. } => None,
        }
    }

    pub fn rewrite_slot(&mut self, rewrite: &mut impl FnMut(Slot) -> Slot) {
        if let Self::Slot(slot) = self {
            *slot = rewrite(*slot);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConstantOperand {
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

impl ConstantOperand {
    /// Admit canonical numeric constants against their complete semantic width.
    /// Float payloads are preserved bit-for-bit, including NaNs and signed zero.
    pub fn numeric_payload(&self, ty: BuiltinType) -> Option<u64> {
        let (number, bits) = match (self, ty) {
            (Self::F32(value), BuiltinType::F32) => return Some(u64::from(value.to_bits())),
            (Self::F64(value), BuiltinType::F64) => return Some(value.to_bits()),
            (Self::I32(value), BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32) => {
                (i128::from(*value), *value as i64 as u64)
            }
            (
                Self::I64(value),
                BuiltinType::I64 | BuiltinType::U8 | BuiltinType::U16 | BuiltinType::U32,
            ) => (i128::from(*value), *value as u64),
            (Self::U64(value), BuiltinType::U64) => (i128::from(*value), *value),
            _ => return None,
        };
        let (min, max) = ty.integer_bounds()?;
        (min..=max).contains(&number).then_some(bits)
    }
}

// Constant-table identity is representation equality, not script equality:
// retain NaN payloads and distinguish signed zero during pooling and validation.
impl PartialEq for ConstantOperand {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unit, Self::Unit) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::I32(a), Self::I32(b)) => a == b,
            (Self::I64(a), Self::I64(b)) => a == b,
            (Self::U64(a), Self::U64(b)) => a == b,
            (Self::F32(a), Self::F32(b)) => a.to_bits() == b.to_bits(),
            (Self::F64(a), Self::F64(b)) => a.to_bits() == b.to_bits(),
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Char(a), Self::Char(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for ConstantOperand {}

impl Hash for ConstantOperand {
    fn hash<H: Hasher>(&self, state: &mut H) {
        mem::discriminant(self).hash(state);
        match self {
            Self::Unit => {}
            Self::Bool(value) => value.hash(state),
            Self::I32(value) => value.hash(state),
            Self::I64(value) => value.hash(state),
            Self::U64(value) => value.hash(state),
            Self::F32(value) => value.to_bits().hash(state),
            Self::F64(value) => value.to_bits().hash(state),
            Self::Str(value) => value.hash(state),
            Self::Char(value) => value.hash(state),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
    Shared {
        module: ModuleRef,
        target: CallableTarget,
        contract: Box<SharedCall>,
    },
    ModuleFunction {
        module: ModuleRef,
        function: FunctionRef,
    },
    Function(FunctionRef),
    InterfaceMethod {
        module: ModuleRef,
        contract: Box<InterfaceCallContract>,
    },
    Native(NativeImportId),
    Slot(Slot),
    ClosureRegister {
        register: Slot,
        params: Vec<ValueType>,
        return_type: ValueType,
    },
    RuntimePrimitive(RuntimePrimitive),
    RuntimeHelper(RuntimeHelper),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeHelper {
    ReflectTypeOf,
    ReflectGetField(String),
    ReflectSetField(String),
    ReflectSetIndex,
    DynamicCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Numeric(NumericOperation),
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    IdentityEq,
    IdentityNotEq,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BytecodeInstruction {
    Await {
        dst: Slot,
        value: Slot,
        future: Ty,
    },
    Convert {
        dst: Slot,
        src: Slot,
        conversion: NumericConversion,
    },
    Numeric {
        dst: Slot,
        operation: NumericOperation,
        lhs: Slot,
        rhs: Option<Slot>,
    },
    Iter {
        dst: Slot,
        value: Option<Slot>,
        ty: Ty,
        op: IterOp,
    },
    LoadConst {
        dst: Slot,
        constant: ConstantId,
    },
    LoadModule {
        dst: Slot,
        slot: ModuleSlot,
    },
    StoreModule {
        slot: ModuleSlot,
        src: Slot,
    },
    Move {
        dst: Slot,
        src: Slot,
    },
    Unary {
        dst: Slot,
        op: UnaryOp,
        operand: Slot,
    },
    Binary {
        dst: Slot,
        op: BinaryOp,
        lhs: Slot,
        rhs: Slot,
    },
    /// Numeric binary computation; canonical constants carry complete source widths.
    BinaryOperands {
        dst: Slot,
        op: BinaryOp,
        lhs: ScalarOperand,
        rhs: ScalarOperand,
    },
    /// Compare and transfer control without publishing a bool. Negation changes
    /// polarity, never the predicate itself (ordered float inverses differ on NaN).
    PredicateBranch {
        op: BinaryOp,
        lhs: ScalarOperand,
        rhs: ScalarOperand,
        negated: bool,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
    Call {
        dst: Option<Slot>,
        callee: CallTarget,
        args: Vec<Slot>,
    },
    MakeTuple {
        dst: Slot,
        elements: Vec<Slot>,
    },
    RangeBound {
        dst: Slot,
        value: Slot,
        range: Ty,
        bound: Ty,
        upper: bool,
    },
    MakeRange {
        dst: Slot,
        start: Option<Slot>,
        end: Option<Slot>,
        ty: Ty,
    },
    RepeatArray {
        element: Ty,
        dst: Slot,
        value: Slot,
        count: Slot,
    },
    MakeArray {
        element: Ty,
        dst: Slot,
        elements: Vec<Slot>,
    },
    /// Capture every resume parameter without entering its body.
    MakeFuture {
        dst: Slot,
        function: FunctionRef,
        arguments: Vec<Slot>,
        future: Ty,
    },
    MakeClosure {
        dst: Slot,
        function: FunctionRef,
        captures: Vec<Slot>,
    },
    MakeCell {
        dst: Slot,
        value: Slot,
    },
    ReadCell {
        dst: Slot,
        cell: Slot,
    },
    WriteCell {
        cell: Slot,
        value: Slot,
    },
    UpcastInterface {
        dst: Slot,
        value: Slot,
        source: NominalTy,
        target: NominalTy,
    },
    MakeInterface {
        dst: Slot,
        value: Slot,
        target: ConstructionTarget,
    },
    MakeStruct {
        dst: Slot,
        structure: StructId,
        arguments: Vec<Ty>,
        fields: Vec<Slot>,
    },
    MakeEnum {
        dst: Slot,
        enumeration: EnumId,
        arguments: Vec<Ty>,
        variant: u32,
        fields: Vec<Slot>,
    },
    TestEnumVariant {
        dst: Slot,
        value: Slot,
        enumeration: EnumId,
        arguments: Vec<Ty>,
        variant: u32,
    },
    ReadEnumPayload {
        dst: Slot,
        value: Slot,
        enumeration: EnumId,
        arguments: Vec<Ty>,
        variant: u32,
        index: u32,
    },
    ReadAggregateField {
        dst: Slot,
        base: Slot,
        field: FieldRef,
    },
    WriteAggregateField {
        base: Slot,
        field: FieldRef,
        value: Slot,
    },
    ReadAggregateIndex {
        dst: Slot,
        base: Slot,
        index: Slot,
    },
    WriteAggregateIndex {
        base: Slot,
        index: Slot,
        value: Slot,
    },
    ReadPath {
        dst: Slot,
        root_or_view: Slot,
        path: PathId,
        dynamic_args: Vec<Slot>,
    },
    SetPath {
        root_or_view: Slot,
        path: PathId,
        dynamic_args: Vec<Slot>,
        value: Slot,
    },
    ModifyPath {
        dst: Option<Slot>,
        root_or_view: Slot,
        path: PathId,
        dynamic_args: Vec<Slot>,
        op: BinaryOp,
        value: Slot,
    },
    MakePathView {
        dst: Slot,
        root_or_view: Slot,
        path: PathId,
        dynamic_args: Vec<Slot>,
    },
    Jump {
        target: JumpTarget,
    },
    Branch {
        cond: Slot,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
    Return(Option<Slot>),
    Unreachable,
}

impl BytecodeInstruction {
    pub(crate) fn layout_arguments(&self) -> Option<&[Ty]> {
        match self {
            Self::MakeStruct { arguments, .. }
            | Self::MakeEnum { arguments, .. }
            | Self::TestEnumVariant { arguments, .. }
            | Self::ReadEnumPayload { arguments, .. } => Some(arguments),
            Self::ReadAggregateField { field, .. } | Self::WriteAggregateField { field, .. } => {
                Some(&field.arguments)
            }
            _ => None,
        }
    }

    pub(crate) fn operand_vector_len(&self) -> usize {
        match self {
            Self::Call { args, .. } => args.len(),
            Self::MakeTuple { elements, .. } | Self::MakeArray { elements, .. } => elements.len(),
            Self::MakeStruct { fields, .. } | Self::MakeEnum { fields, .. } => fields.len(),
            Self::ReadPath { dynamic_args, .. }
            | Self::SetPath { dynamic_args, .. }
            | Self::ModifyPath { dynamic_args, .. }
            | Self::MakePathView { dynamic_args, .. } => dynamic_args.len(),
            _ => 0,
        }
    }
}

mod mapping;
mod operands;
