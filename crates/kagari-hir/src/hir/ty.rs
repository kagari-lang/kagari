use smallvec::SmallVec;

use crate::hir::TypeRefId;

#[derive(Debug, Clone)]
pub struct TypeData {
    pub kind: TypeKind,
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    Named(String),
    Generic {
        name: String,
        args: TypeBuffer,
        bindings: Vec<(String, TypeRefId)>,
        positional_after_binding: bool,
    },
    Projection {
        receiver: TypeRefId,
        trait_ref: TypeRefId,
        member: String,
    },
    Tuple(TypeBuffer),
    Array(TypeRefId),
    Function {
        params: TypeBuffer,
        result: TypeRefId,
    },
}

pub type TypeBuffer = SmallVec<[TypeRefId; 4]>;
