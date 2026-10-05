use crate::hir::{
    ids::{BlockId, FunctionId, ParamId, TypeRefId},
    item::behavior::{GenericParamBuffer, TraitBoundBuffer},
    writeability::Writeability,
};
use kagari_types::visibility::Visibility;

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
    /// Absent for interface requirements and installed native declarations.
    pub body: Option<BlockId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    User,
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
