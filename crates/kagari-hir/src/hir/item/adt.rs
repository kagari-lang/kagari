use crate::hir::{
    ids::{EnumId, FieldId, ImplId, MethodId, OpaqueTypeId, StructId, TypeRefId, VariantId},
    item::behavior::{GenericParam, TraitBound, TraitRef},
    writeability::Writeability,
};
use kagari_types::visibility::Visibility;

/// A declaration whose storage is supplied by an installed native provider.
/// Its representation is checked separately from its ordinary generic syntax.
#[derive(Debug, Clone)]
pub struct OpaqueType {
    pub id: OpaqueTypeId,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<TraitBound>,
    pub trait_bounds: Vec<TraitRef>,
    pub definition: Option<TypeRefId>,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub id: StructId,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub fields: FieldBuffer,
    pub methods: Vec<MethodId>,
    pub impls: Vec<ImplId>,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub id: FieldId,
    pub visibility: Visibility,
    pub writeability: Writeability,
    pub name: String,
    pub ty: TypeRefId,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub id: EnumId,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub variants: VariantBuffer,
    pub methods: Vec<MethodId>,
    pub impls: Vec<ImplId>,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub id: VariantId,
    pub name: String,
    pub payload: Vec<TypeRefId>,
}

pub type StructBuffer = Vec<Struct>;
pub type FieldBuffer = Vec<Field>;
pub type EnumBuffer = Vec<Enum>;
pub type VariantBuffer = Vec<Variant>;
