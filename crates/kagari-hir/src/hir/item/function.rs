use smallvec::SmallVec;

use crate::hir::{BlockId, FunctionId, ParamId, TypeRefId};

#[derive(Debug, Clone)]
pub struct Function {
    pub id: FunctionId,
    pub name: String,
    pub params: ParamBuffer,
    pub return_type: Option<TypeRefId>,
    pub body: BlockId,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub id: ParamId,
    pub name: String,
    pub ty: TypeRefId,
}

pub type FunctionBuffer = SmallVec<[Function; 8]>;
pub type ParamBuffer = SmallVec<[Param; 4]>;
