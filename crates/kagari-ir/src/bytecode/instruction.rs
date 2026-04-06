#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Register(u16);

impl Register {
    pub fn new(index: usize) -> Self {
        Self(index as u16)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalSlot(u16);

impl LocalSlot {
    pub fn new(index: usize) -> Self {
        Self(index as u16)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleSlot(u16);

impl ModuleSlot {
    pub fn new(index: usize) -> Self {
        Self(index as u16)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JumpTarget(u32);

impl JumpTarget {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionRef(u32);

impl FunctionRef {
    pub fn new(index: usize) -> Self {
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstantOperand {
    Unit,
    Bool(bool),
    I32(i32),
    F32(f32),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
    Function(FunctionRef),
    Register(Register),
    RuntimeHelper(RuntimeHelper),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHelper {
    HostFunction(String),
    ReflectTypeOf,
    ReflectGetField(String),
    ReflectSetField(String),
    ReflectSetIndex,
    DynamicCall,
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
}

#[derive(Debug, Clone, PartialEq)]
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
        args: Vec<Register>,
    },
    MakeTuple {
        dst: Register,
        elements: Vec<Register>,
    },
    MakeArray {
        dst: Register,
        elements: Vec<Register>,
    },
    MakeStruct {
        dst: Register,
        name: String,
        fields: Vec<StructFieldInit>,
    },
    ReadField {
        dst: Register,
        base: Register,
        name: String,
    },
    ReadIndex {
        dst: Register,
        base: Register,
        index: Register,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructFieldInit {
    pub name: String,
    pub value: Register,
}
