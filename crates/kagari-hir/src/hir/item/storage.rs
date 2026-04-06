use crate::hir::{ConstId, EnumId, ExprId, FunctionId, StaticId, StructId, TypeRefId};

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
    pub mutable: bool,
    pub name: String,
    pub ty: Option<TypeRefId>,
    pub initializer: ExprId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportItem {
    Function(FunctionId),
    Const(ConstId),
    Static(StaticId),
    Struct(StructId),
    Enum(EnumId),
}

#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,
    pub item: ExportItem,
}

pub type ConstBuffer = Vec<ConstItem>;
pub type StaticBuffer = Vec<StaticItem>;
pub type ExportBuffer = Vec<Export>;
