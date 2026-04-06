use smallvec::SmallVec;

use crate::hir::TypeRefId;

#[derive(Debug, Clone)]
pub struct TypeData {
    pub kind: TypeKind,
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    Named(String),
    Tuple(TypeBuffer),
    Array(TypeRefId),
}

pub type TypeBuffer = SmallVec<[TypeRefId; 4]>;
