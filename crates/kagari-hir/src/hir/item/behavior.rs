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
    pub id: TraitId,
    pub visibility: Visibility,
    pub name: String,
    pub methods: TraitMethodBuffer,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub id: TraitMethodId,
    pub name: String,
    pub receiver: ReceiverKind,
    pub function: FunctionId,
}

#[derive(Debug, Clone)]
pub struct Impl {
    pub id: ImplId,
    pub trait_ref: Option<String>,
    pub for_type: Option<TypeRefId>,
    pub methods: ImplMethodBuffer,
}

#[derive(Debug, Clone)]
pub struct ImplMethod {
    pub trait_method: TraitMethodId,
    pub function: FunctionId,
}

pub type MethodBuffer = Vec<Method>;
pub type TraitBuffer = Vec<TraitDef>;
pub type TraitMethodBuffer = Vec<TraitMethod>;
pub type ImplBuffer = Vec<Impl>;
pub type ImplMethodBuffer = Vec<ImplMethod>;
