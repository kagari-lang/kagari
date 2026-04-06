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

pub type FunctionBuffer = Vec<Function>;
pub type ParamBuffer = Vec<Param>;
