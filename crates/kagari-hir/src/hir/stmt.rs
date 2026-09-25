use smallvec::SmallVec;

use crate::hir::{BlockId, ExprId, LocalId, PatternId, PlaceId, StmtId, TypeRefId, Writeability};

#[derive(Debug, Clone)]
pub struct BlockData {
    pub statements: StmtBuffer,
    pub tail_expr: Option<ExprId>,
}

#[derive(Debug, Clone)]
pub struct StmtData {
    pub kind: StmtKind,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Binding {
        local: LocalId,
        writeability: Writeability,
        name: String,
        ty: Option<TypeRefId>,
        initializer: ExprId,
    },
    Assign {
        target: PlaceId,
        op: Option<crate::hir::BinaryOp>,
        value: ExprId,
    },
    Return {
        expr: Option<ExprId>,
    },
    While {
        condition: crate::hir::Condition,
        body: BlockId,
    },
    Loop {
        body: BlockId,
    },
    For {
        pattern: PatternId,
        iterable: ExprId,
        body: BlockId,
    },
    Break,
    BreakValue(ExprId),
    Continue,
    Expr(ExprId),
}

pub type StmtBuffer = SmallVec<[StmtId; 8]>;
