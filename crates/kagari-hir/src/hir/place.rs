use crate::hir::{ExprId, PlaceId};

#[derive(Debug, Clone)]
pub struct PlaceData {
    pub kind: PlaceKind,
}

#[derive(Debug, Clone)]
pub enum PlaceKind {
    Name(String),
    Field { base: PlaceId, name: String },
    Index { base: PlaceId, index: ExprId },
}
