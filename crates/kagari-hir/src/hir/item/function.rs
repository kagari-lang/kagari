use crate::hir::{BlockId, FunctionId, ParamId, TypeRefId};

use super::Visibility;

#[derive(Debug, Clone)]
pub struct Function {
    pub id: FunctionId,
    pub kind: FunctionKind,
    pub visibility: Visibility,
    pub name: String,
    pub params: ParamBuffer,
    pub return_type: Option<TypeRefId>,
    pub body: BlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    User,
    ModuleInit,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub id: ParamId,
    pub name: String,
    pub ty: TypeRefId,
}

pub type FunctionBuffer = Vec<Function>;
pub type ParamBuffer = Vec<Param>;
