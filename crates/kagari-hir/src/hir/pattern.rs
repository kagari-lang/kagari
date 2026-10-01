use crate::hir::{
    expr::literal::Literal,
    ids::{LocalId, PatternId},
};

#[derive(Debug, Clone)]
pub struct PatternData {
    pub kind: PatternKind,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
    Wildcard,
    Or(Vec<PatternId>),
    Range {
        start: PatternBound,
        end: PatternBound,
        inclusive: bool,
    },
    Name {
        name: String,
        local: LocalId,
    },
    Literal(Literal),
    Tuple(Vec<PatternId>),
    Struct {
        path: String,
        fields: Vec<PatternField>,
    },
    EnumVariant {
        path: String,
        fields: Vec<PatternId>,
    },
}

#[derive(Debug, Clone)]
pub enum PatternBound {
    Literal(Literal),
    Path(String),
}

#[derive(Debug, Clone)]
pub struct PatternField {
    pub name: String,
    pub pattern: PatternId,
}
