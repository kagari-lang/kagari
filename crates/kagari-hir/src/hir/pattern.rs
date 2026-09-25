use crate::hir::{LocalId, PatternId, expr::Literal};

#[derive(Debug, Clone)]
pub struct PatternData {
    pub kind: PatternKind,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
    Wildcard,
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
pub struct PatternField {
    pub name: String,
    pub pattern: PatternId,
}

impl PatternKind {
    pub fn is_irrefutable(&self) -> bool {
        matches!(self, Self::Wildcard | Self::Name { .. })
    }
}
