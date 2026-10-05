use crate::hir::ids::{
    ConstId, EnumId, ExprId, FunctionId, ImplId, ModuleId, OpaqueTypeId, StructId, TraitId,
    TypeRefId, VariantId,
};
use kagari_types::visibility::Visibility;

#[derive(Debug, Clone)]
pub struct ConstItem {
    pub owner: Option<ConstOwner>,
    pub id: ConstId,
    pub visibility: Visibility,
    pub name: String,
    pub ty: Option<TypeRefId>,
    pub initializer: ExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstOwner {
    Trait(TraitId),
    Impl(ImplId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportItem {
    OpaqueType(OpaqueTypeId),
    Function(FunctionId),
    Const(ConstId),
    Module(ModuleId),
    Import(usize),
    Struct(StructId),
    Enum(EnumId),
    Variant(VariantId),
    Trait(TraitId),
}

#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,
    pub item: ExportItem,
}

pub type ConstBuffer = Vec<ConstItem>;
pub type ExportBuffer = Vec<Export>;
