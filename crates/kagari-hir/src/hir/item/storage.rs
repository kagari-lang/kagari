use crate::hir::{
    ConstId, EnumId, ExprId, FunctionId, ModuleId, StaticId, StructId, TraitId, TypeRefId,
    Writeability,
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

#[derive(Debug, Clone)]
pub struct StaticItem {
    pub id: StaticId,
    pub visibility: Visibility,
    pub writeability: Writeability,
    pub name: String,
    pub ty: Option<TypeRefId>,
    pub initializer: ExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportItem {
    Function(FunctionId),
    Const(ConstId),
    Static(StaticId),
    Module(ModuleId),
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
pub type StaticBuffer = Vec<StaticItem>;
pub type ExportBuffer = Vec<Export>;
