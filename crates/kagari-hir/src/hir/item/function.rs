use crate::hir::{
    BlockId, FunctionId, GenericParamBuffer, ParamId, TraitBoundBuffer, TypeRefId, Writeability,
};

use super::Visibility;

#[derive(Debug, Clone)]
pub struct Function {
    pub id: FunctionId,
    pub kind: FunctionKind,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: GenericParamBuffer,
    pub bounds: TraitBoundBuffer,
    pub params: ParamBuffer,
    pub return_type: Option<TypeRefId>,
    pub body: BlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    User,
    ModuleInit,
    TraitMethod,
    ImplMethod,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub id: ParamId,
    pub writeability: Writeability,
    pub name: String,
    pub ty: TypeRefId,
}

pub type FunctionBuffer = Vec<Function>;
pub type ParamBuffer = Vec<Param>;
