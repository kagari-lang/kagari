use kagari_hir::builtin::surface::StandardIntrinsic;
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
pub struct FunctionRef(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HostImportId(u32);
impl HostImportId {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl FunctionRef {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl Default for FunctionRef {
    fn default() -> Self {
        Self::new(0)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldRef {
    pub structure: StructId,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConstantOperand {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    F32(f32),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallTarget {
    ModuleFunction {
        module: super::ModuleRef,
        function: FunctionRef,
    },
    Function(FunctionRef),
    InterfaceMethod {
        module: super::ModuleRef,
        interface: crate::module::abi::NominalAbiType,
        method_slot: u32,
    },
    HostFunction(HostImportId),
    Register(Register),
    ClosureRegister {
        register: Register,
        #[serde(deserialize_with = "crate::decode_limits::table")]
        params: Vec<crate::module::ValueType>,
        return_type: crate::module::ValueType,
    },
    StandardIntrinsic(StandardIntrinsic),
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
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BytecodeInstruction {
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
        callee: CallTarget,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
        args: Vec<Register>,
    },
    BeginIteration {
        collection: Register,
    },
    EndIteration,
    MakeTuple {
        dst: Register,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
        elements: Vec<Register>,
    },
    MakeArray {
        dst: Register,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
        elements: Vec<Register>,
    },
    MakeClosure {
        dst: Register,
        function: FunctionRef,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
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
    MakeInterface {
        dst: Register,
        value: Register,
        module: super::ModuleRef,
        implementation: InterfaceTableRef,
    },
    MakeStruct {
        dst: Register,
        structure: StructId,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
        fields: Vec<Register>,
    },
    MakeEnum {
        dst: Register,
        enumeration: EnumId,
        variant: u32,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
        fields: Vec<Register>,
    },
    TestEnumVariant {
        dst: Register,
        value: Register,
        enumeration: EnumId,
        variant: u32,
    },
    ReadEnumPayload {
        dst: Register,
        value: Register,
        enumeration: EnumId,
        variant: u32,
        index: u32,
    },
    ReadAggregateField {
        dst: Register,
        base: Register,
        field: FieldRef,
    },
    WriteAggregateField {
        base: Register,
        field: FieldRef,
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
        #[serde(deserialize_with = "crate::decode_limits::operands")]
        dynamic_args: Vec<Register>,
    },
    SetPath {
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
        dynamic_args: Vec<Register>,
        value: Register,
    },
    ModifyPath {
        dst: Option<Register>,
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
        dynamic_args: Vec<Register>,
        op: BinaryOp,
        value: Register,
    },
    MakePathView {
        dst: Register,
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "crate::decode_limits::operands")]
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
