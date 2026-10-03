use crate::{module::CallableTarget, program::ModuleRef};
use kagari_abi::{
    callable::{interface::InterfaceCallContract, shared::SharedCall},
    ids::FunctionRef,
    numeric::{NumericConversion, NumericOperation},
    operations::{IterOp, StandardEnumOp},
    representation::ValueType,
    standard::RuntimePrimitive,
    types::{AbiType, NominalAbiType},
};
use kagari_common::identity::DefinitionPath;
use kagari_common::identity::reference::DefinitionReference;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Register(u16);

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

impl Register {
    pub fn new(index: usize) -> Self {
        Self(index as u16)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalSlot(u16);

impl LocalSlot {
    pub fn new(index: usize) -> Self {
        Self(index as u16)
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct FieldRef<I = DefinitionPath> {
    pub structure: StructId,
    #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
    pub arguments: Vec<AbiType<I>>,
    pub slot: u32,
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
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub enum CallTarget<I = DefinitionPath> {
    Shared {
        module: ModuleRef,
        target: CallableTarget,
        contract: Box<SharedCall<I>>,
    },
    ModuleFunction {
        module: ModuleRef,
        function: FunctionRef,
    },
    Function(FunctionRef),
    InterfaceMethod {
        module: ModuleRef,
        contract: Box<InterfaceCallContract<I>>,
    },
    Native(NativeImportId),
    Register(Register),
    ClosureRegister {
        register: Register,
        #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub enum BytecodeInstruction<I = DefinitionPath> {
    Convert {
        dst: Register,
        src: Register,
        conversion: NumericConversion,
    },
    Numeric {
        dst: Register,
        operation: NumericOperation,
        lhs: Register,
        rhs: Option<Register>,
    },
    MapResultError {
        dst: Register,
        original: Register,
        error: Register,
        ty: AbiType<I>,
    },
    Iter {
        dst: Register,
        value: Option<Register>,
        ty: AbiType<I>,
        op: IterOp,
    },
    StandardEnum {
        dst: Register,
        value: Option<Register>,
        ty: AbiType<I>,
        op: StandardEnumOp,
    },
    LoadConst {
        dst: Register,
        constant: ConstantOperand,
    },
    LoadLocal {
        dst: Register,
        local: LocalSlot,
    },
    LoadModule {
        dst: Register,
        slot: ModuleSlot,
    },
    StoreLocal {
        local: LocalSlot,
        src: Register,
    },
    StoreModule {
        slot: ModuleSlot,
        src: Register,
    },
    Move {
        dst: Register,
        src: Register,
    },
    Unary {
        dst: Register,
        op: UnaryOp,
        operand: Register,
    },
    Binary {
        dst: Register,
        op: BinaryOp,
        lhs: Register,
        rhs: Register,
    },
    Call {
        dst: Option<Register>,
        callee: CallTarget<I>,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        args: Vec<Register>,
    },
    BeginIteration {
        collection: Register,
    },
    EndIteration,
    MakeTuple {
        dst: Register,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        elements: Vec<Register>,
    },
    RangeBound {
        dst: Register,
        value: Register,
        range: AbiType<I>,
        bound: AbiType<I>,
        upper: bool,
    },
    MakeRange {
        dst: Register,
        start: Option<Register>,
        end: Option<Register>,
        ty: AbiType<I>,
    },
    RepeatArray {
        element: AbiType<I>,
        dst: Register,
        value: Register,
        count: Register,
    },
    MakeArray {
        element: AbiType<I>,
        dst: Register,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        elements: Vec<Register>,
    },
    MakeClosure {
        dst: Register,
        function: FunctionRef,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        captures: Vec<Register>,
    },
    MakeCell {
        dst: Register,
        value: Register,
    },
    ReadCell {
        dst: Register,
        cell: Register,
    },
    WriteCell {
        cell: Register,
        value: Register,
    },
    UpcastInterface {
        dst: Register,
        value: Register,
        source: NominalAbiType<I>,
        target: NominalAbiType<I>,
    },
    MakeInterface {
        dst: Register,
        value: Register,
        module: ModuleRef,
        implementation: InterfaceTableRef,
        #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
        arguments: Vec<AbiType<I>>,
    },
    MakeStruct {
        dst: Register,
        structure: StructId,
        #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
        arguments: Vec<AbiType<I>>,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        fields: Vec<Register>,
    },
    MakeEnum {
        dst: Register,
        enumeration: EnumId,
        #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
        arguments: Vec<AbiType<I>>,
        variant: u32,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        fields: Vec<Register>,
    },
    TestEnumVariant {
        dst: Register,
        value: Register,
        enumeration: EnumId,
        #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
        arguments: Vec<AbiType<I>>,
        variant: u32,
    },
    ReadEnumPayload {
        dst: Register,
        value: Register,
        enumeration: EnumId,
        #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
        arguments: Vec<AbiType<I>>,
        variant: u32,
        index: u32,
    },
    ReadAggregateField {
        dst: Register,
        base: Register,
        field: FieldRef<I>,
    },
    WriteAggregateField {
        base: Register,
        field: FieldRef<I>,
        value: Register,
    },
    ReadAggregateIndex {
        dst: Register,
        base: Register,
        index: Register,
    },
    WriteAggregateIndex {
        base: Register,
        index: Register,
        value: Register,
    },
    ReadPath {
        dst: Register,
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        dynamic_args: Vec<Register>,
    },
    SetPath {
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        dynamic_args: Vec<Register>,
        value: Register,
    },
    ModifyPath {
        dst: Option<Register>,
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        dynamic_args: Vec<Register>,
        op: BinaryOp,
        value: Register,
    },
    MakePathView {
        dst: Register,
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "kagari_abi::decode_limits::operands")]
        dynamic_args: Vec<Register>,
    },
    Jump {
        target: JumpTarget,
    },
    Branch {
        cond: Register,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
    Return(Option<Register>),
    Unreachable,
}

impl BytecodeInstruction {
    pub(crate) fn layout_arguments(&self) -> Option<&[AbiType]> {
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
