use crate::{
    builtin::surface,
    hir::{ConstId, EnumId, ExprId, FunctionId, ModuleId, StructId, TraitId, TypeRefId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Debug, Clone)]
pub struct ConstItem {
    pub id: ConstId,
    pub visibility: Visibility,
    pub name: String,
    pub ty: Option<TypeRefId>,
    pub initializer: ExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportItem {
    Function(FunctionId),
    Const(ConstId),
    Module(ModuleId),
    StandardModule(surface::StandardModule),
    StandardFunction(surface::StandardIntrinsic),
    Struct(StructId),
    Enum(EnumId),
    Trait(TraitId),
}

#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,
    pub item: ExportItem,
}

pub type ConstBuffer = Vec<ConstItem>;
pub type ExportBuffer = Vec<Export>;
