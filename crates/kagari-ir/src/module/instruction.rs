use kagari_hir::hir;
use smallvec::SmallVec;

use crate::module::ids::{BlockId, LocalId, ModuleSlotId, TempId};

#[derive(Debug, Clone)]
pub enum Instruction {
    LoadConst {
        dst: TempId,
        constant: Constant,
    },
    LoadLocal {
        dst: TempId,
        local: LocalId,
    },
    LoadModule {
        dst: TempId,
        slot: ModuleSlotId,
    },
    StoreLocal {
        local: LocalId,
        src: TempId,
    },
    StoreModule {
        slot: ModuleSlotId,
        src: TempId,
    },
    Move {
        dst: TempId,
        src: TempId,
    },
    Unary {
        dst: TempId,
        op: UnaryOp,
        operand: TempId,
    },
    Binary {
        dst: TempId,
        op: BinaryOp,
        lhs: TempId,
        rhs: TempId,
    },
    Call {
        dst: Option<TempId>,
        callee: CallTarget,
        args: TempIdBuffer,
    },
    MakeTuple {
        dst: TempId,
        elements: TempIdBuffer,
    },
    MakeArray {
        dst: TempId,
        elements: TempIdBuffer,
    },
    MakeStruct {
        dst: TempId,
        name: String,
        fields: StructFieldInitBuffer,
    },
    ReadField {
        dst: TempId,
        base: TempId,
        name: String,
    },
    ReadIndex {
        dst: TempId,
        base: TempId,
        index: TempId,
    },
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Return(Option<TempId>),
    Jump(BlockId),
    Branch {
        cond: TempId,
        then_block: BlockId,
        else_block: BlockId,
    },
    Unreachable,
}

#[derive(Debug, Clone)]
pub enum CallTarget {
    Function(hir::FunctionId),
    Temp(TempId),
    RuntimeHelper(RuntimeHelper),
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
    pub value: TempId,
}

pub type InstructionBuffer = Vec<Instruction>;
pub type TempIdBuffer = SmallVec<[TempId; 4]>;
pub type StructFieldInitBuffer = SmallVec<[StructFieldInit; 4]>;
