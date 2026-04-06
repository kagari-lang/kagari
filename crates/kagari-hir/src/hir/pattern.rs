use crate::hir::{LocalId, expr::Literal};

#[derive(Debug, Clone)]
pub struct PatternData {
    pub kind: PatternKind,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
    Wildcard,
    Name { name: String, local: LocalId },
    Literal(Literal),
}
