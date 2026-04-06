use crate::hir::expr::Literal;

#[derive(Debug, Clone)]
pub struct PatternData {
    pub kind: PatternKind,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
    Wildcard,
    Name(String),
    Literal(Literal),
}
