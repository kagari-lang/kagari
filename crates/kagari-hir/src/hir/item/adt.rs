use crate::hir::{EnumId, ImplId, MethodId, StructId, TypeRefId};

use super::Visibility;

#[derive(Debug, Clone)]
pub struct Struct {
    pub id: StructId,
    pub visibility: Visibility,
    pub name: String,
    pub fields: FieldBuffer,
    pub methods: Vec<MethodId>,
    pub impls: Vec<ImplId>,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: TypeRefId,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub id: EnumId,
    pub visibility: Visibility,
    pub name: String,
    pub variants: VariantBuffer,
    pub methods: Vec<MethodId>,
    pub impls: Vec<ImplId>,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
}

pub type StructBuffer = Vec<Struct>;
pub type FieldBuffer = Vec<Field>;
pub type EnumBuffer = Vec<Enum>;
pub type VariantBuffer = Vec<Variant>;
