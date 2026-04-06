use smallvec::SmallVec;

use crate::hir::{BlockId, ExprId, LocalId, PlaceId, StmtId, TypeRefId};

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
    Let {
        local: LocalId,
        mutable: bool,
        name: String,
        ty: Option<TypeRefId>,
        initializer: ExprId,
    },
    Assign {
        target: PlaceId,
        value: ExprId,
    },
    Return {
        expr: Option<ExprId>,
    },
    While {
        condition: ExprId,
        body: BlockId,
    },
    Loop {
        body: BlockId,
    },
    Break,
    Continue,
    Expr(ExprId),
}

pub type StmtBuffer = SmallVec<[StmtId; 8]>;
