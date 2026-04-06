mod adt;
mod function;
mod storage;

pub use adt::{Enum, EnumBuffer, Field, FieldBuffer, Struct, StructBuffer, Variant, VariantBuffer};
pub use function::{Function, FunctionBuffer, FunctionKind, Param, ParamBuffer};
pub use storage::{
    ConstBuffer, ConstItem, Export, ExportBuffer, ExportItem, StaticBuffer, StaticItem, Visibility,
};

use crate::hir::{
    BlockData, BlockId, Body, ConstId, EnumId, ExprData, ExprId, FunctionId, PatternData,
    PatternId, PlaceData, PlaceId, StaticId, StmtData, StmtId, StructId, TypeData, TypeRefId,
};

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub items: ItemBuffer,
    pub exports: ExportBuffer,
    pub module_init: Option<FunctionId>,
    pub functions: FunctionBuffer,
    pub consts: ConstBuffer,
    pub statics: StaticBuffer,
    pub structs: StructBuffer,
    pub enums: EnumBuffer,
    pub body: Body,
}

impl Module {
    pub fn block(&self, id: BlockId) -> &BlockData {
        self.body.block(id)
    }

    pub fn stmt(&self, id: StmtId) -> &StmtData {
        self.body.stmt(id)
    }

    pub fn expr(&self, id: ExprId) -> &ExprData {
        self.body.expr(id)
    }

    pub fn place(&self, id: PlaceId) -> &PlaceData {
        self.body.place(id)
    }

    pub fn pattern(&self, id: PatternId) -> &PatternData {
        self.body.pattern(id)
    }

    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        self.body.type_ref(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Function(FunctionId),
    Const(ConstId),
    Static(StaticId),
    Struct(StructId),
    Enum(EnumId),
}

pub type ItemBuffer = Vec<Item>;
