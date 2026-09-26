use crate::hir::{FunctionId, ImplId, MethodId, StructId, TraitId, TraitMethodId, TypeRefId};

use super::Visibility;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverKind {
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodOwner {
    Struct(StructId),
    Trait(TraitId),
}

#[derive(Debug, Clone)]
pub struct Method {
    pub id: MethodId,
    pub owner: MethodOwner,
    pub visibility: Visibility,
    pub name: String,
    pub receiver: ReceiverKind,
    pub function: FunctionId,
}

#[derive(Debug, Clone)]
pub struct TraitDef {
    pub associated_consts: Vec<AssociatedConst>,
    pub id: TraitId,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: GenericParamBuffer,
    pub supertraits: TraitRefBuffer,
    pub methods: TraitMethodBuffer,
    pub associated_types: Vec<AssociatedType>,
}

#[derive(Debug, Clone)]
pub struct AssociatedConst {
    pub name: String,
    pub name_ref: TypeRefId,
    pub ty: TypeRefId,
    pub initializer: Option<crate::hir::ConstId>,
}

#[derive(Debug, Clone)]
pub struct AssociatedType {
    pub generic_params: GenericParamBuffer,
    pub parameter_bounds: TraitBoundBuffer,
    pub name: String,
    pub name_ref: TypeRefId,
    pub ty: Option<TypeRefId>,
    pub bounds: TraitRefBuffer,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub has_default: bool,
    pub id: TraitMethodId,
    pub name: String,
    pub receiver: ReceiverKind,
    pub function: FunctionId,
}

#[derive(Debug, Clone)]
pub struct Impl {
    pub associated_consts: Vec<AssociatedConst>,
    pub id: ImplId,
    pub generic_params: GenericParamBuffer,
    pub trait_ref: Option<TraitRef>,
    pub for_type: Option<TypeRefId>,
    pub bounds: TraitBoundBuffer,
    pub methods: ImplMethodBuffer,
    pub associated_types: Vec<AssociatedType>,
}

#[derive(Debug, Clone)]
pub struct ImplMethod {
    pub name: String,
    pub function: FunctionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParam {
    pub id: crate::hir::GenericParamId,
    pub name: String,
    pub bounds: TraitRefBuffer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitBound {
    pub target: String,
    pub target_ref: TypeRefId,
    pub traits: TraitRefBuffer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitRef {
    pub ty: TypeRefId,
}

pub type MethodBuffer = Vec<Method>;
pub type TraitBuffer = Vec<TraitDef>;
pub type TraitMethodBuffer = Vec<TraitMethod>;
pub type ImplBuffer = Vec<Impl>;
pub type ImplMethodBuffer = Vec<ImplMethod>;
pub type GenericParamBuffer = Vec<GenericParam>;
pub type TraitBoundBuffer = Vec<TraitBound>;
pub type TraitRefBuffer = Vec<TraitRef>;
