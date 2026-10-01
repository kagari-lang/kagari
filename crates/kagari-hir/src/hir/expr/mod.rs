pub mod literal;
use crate::hir::{
    expr::{
        literal::Literal,
        ops::{BinaryOp, PrefixOp},
    },
    ids::{BlockId, ExprId, LocalId, PatternId, TypeRefId},
    ty::TypeBuffer,
};

pub mod ops;

use smallvec::SmallVec;

#[derive(Debug, Clone)]
pub struct ExprData {
    pub kind: ExprKind,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Missing,
    Cast {
        expr: ExprId,
        target: TypeRefId,
    },
    InterpolatedString(ExprBuffer),
    FormatPart {
        expr: ExprId,
        debug: bool,
    },
    Name {
        name: String,
        explicit_type: Option<TypeRefId>,
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
        start: Option<ExprId>,
        end: Option<ExprId>,
        inclusive: bool,
    },
    Call {
        callee: ExprId,
        args: ExprBuffer,
        type_args: Option<TypeBuffer>,
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
        explicit_type: Option<TypeRefId>,
        fields: FieldInitBuffer,
    },
    Tuple(ExprBuffer),
    Array(ExprBuffer),
    ArrayRepeat {
        value: ExprId,
        count: ExprId,
    },
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
