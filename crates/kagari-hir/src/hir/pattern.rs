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

impl PatternKind {
    pub fn is_irrefutable(&self) -> bool {
        matches!(self, Self::Wildcard | Self::Name { .. })
    }
}
