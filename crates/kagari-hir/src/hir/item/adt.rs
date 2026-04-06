use smallvec::SmallVec;

use crate::hir::{EnumId, StructId, TypeRefId};

#[derive(Debug, Clone)]
pub struct Struct {
    pub id: StructId,
    pub name: String,
    pub fields: FieldBuffer,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: TypeRefId,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub id: EnumId,
    pub name: String,
    pub variants: VariantBuffer,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
}

pub type StructBuffer = SmallVec<[Struct; 8]>;
pub type FieldBuffer = SmallVec<[Field; 8]>;
pub type EnumBuffer = SmallVec<[Enum; 8]>;
pub type VariantBuffer = SmallVec<[Variant; 8]>;
