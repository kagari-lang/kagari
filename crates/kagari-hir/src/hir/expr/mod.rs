mod literal;
mod ops;

pub use literal::{Literal, LiteralKind};
pub use ops::{BinaryOp, PrefixOp};

use smallvec::SmallVec;

use crate::hir::{BlockId, ExprId, LocalId, PatternId, TypeRefId};

#[derive(Debug, Clone)]
pub struct ExprData {
    pub kind: ExprKind,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Missing,
    Name {
        name: String,
        explicit_type: Option<super::TypeRefId>,
    },
    Literal(Literal),
    Propagate {
        expr: ExprId,
    },
    Prefix {
        op: PrefixOp,
        expr: ExprId,
    },
    Binary {
        lhs: ExprId,
        op: BinaryOp,
        rhs: ExprId,
    },
    Range {
        start: ExprId,
        end: ExprId,
        inclusive: bool,
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
        condition: Condition,
        then_branch: BlockId,
        else_branch: Option<ExprId>,
    },
    Match {
        scrutinee: ExprId,
        arms: MatchArmBuffer,
    },
    Loop {
        body: BlockId,
    },
    Closure {
        params: Vec<ClosureParam>,
        body: ExprId,
    },
    StructInit {
        path: String,
        explicit_type: Option<super::TypeRefId>,
        fields: FieldInitBuffer,
    },
    Tuple(ExprBuffer),
    Array(ExprBuffer),
    Block(BlockId),
}

#[derive(Debug, Clone)]
pub enum Condition {
    Expr(ExprId),
    Binding {
        pattern: PatternId,
        initializer: ExprId,
    },
}

impl Condition {
    pub fn value(&self) -> ExprId {
        match self {
            Self::Expr(expr) => *expr,
            Self::Binding { initializer, .. } => *initializer,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: PatternId,
    pub guard: Option<ExprId>,
    pub expr: ExprId,
}

#[derive(Debug, Clone)]
pub struct FieldInit {
    pub name: String,
    pub value: ExprId,
}

#[derive(Debug, Clone)]
pub struct ClosureParam {
    pub name: String,
    pub local: LocalId,
    pub ty: Option<TypeRefId>,
}

pub type ExprBuffer = SmallVec<[ExprId; 4]>;
pub type MatchArmBuffer = SmallVec<[MatchArm; 4]>;
pub type FieldInitBuffer = SmallVec<[FieldInit; 4]>;
