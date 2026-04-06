mod literal;
mod ops;

pub use literal::{Literal, LiteralKind};
pub use ops::{BinaryOp, PrefixOp};

use smallvec::SmallVec;

use crate::hir::{BlockId, ExprId, PatternId};

#[derive(Debug, Clone)]
pub struct ExprData {
    pub kind: ExprKind,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Name(String),
    Literal(Literal),
    Prefix {
        op: PrefixOp,
        expr: ExprId,
    },
    Binary {
        lhs: ExprId,
        op: BinaryOp,
        rhs: ExprId,
    },
    Call {
        callee: ExprId,
        args: ExprBuffer,
    },
    Field {
        receiver: ExprId,
        name: String,
    },
    Index {
        receiver: ExprId,
        index: ExprId,
    },
    If {
        condition: ExprId,
        then_branch: BlockId,
        else_branch: Option<ExprId>,
    },
    Match {
        scrutinee: ExprId,
        arms: MatchArmBuffer,
    },
    StructInit {
        path: String,
        fields: FieldInitBuffer,
    },
    Tuple(ExprBuffer),
    Array(ExprBuffer),
    Block(BlockId),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: PatternId,
    pub expr: ExprId,
}

#[derive(Debug, Clone)]
pub struct FieldInit {
    pub name: String,
    pub value: ExprId,
}

pub type ExprBuffer = SmallVec<[ExprId; 4]>;
pub type MatchArmBuffer = SmallVec<[MatchArm; 4]>;
pub type FieldInitBuffer = SmallVec<[FieldInit; 4]>;
