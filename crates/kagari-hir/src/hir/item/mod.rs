mod adt;
mod behavior;
mod function;
mod module;
mod storage;

pub use adt::{Enum, EnumBuffer, Field, FieldBuffer, Struct, StructBuffer, Variant, VariantBuffer};
pub use behavior::{
    Impl, ImplBuffer, ImplMethod, ImplMethodBuffer, Method, MethodBuffer, MethodOwner,
    ReceiverKind, TraitBuffer, TraitDef, TraitMethod, TraitMethodBuffer,
};
pub use function::{Function, FunctionBuffer, FunctionKind, Param, ParamBuffer};
pub use module::{ModuleDecl, ModuleDeclBuffer};
pub use storage::{
    ConstBuffer, ConstItem, Export, ExportBuffer, ExportItem, StaticBuffer, StaticItem, Visibility,
};

use crate::hir::{
    BlockData, BlockId, Body, ConstId, EnumId, ExprData, ExprId, FunctionId, ImplId, ModuleId,
    PatternData, PatternId, PlaceData, PlaceId, StaticId, StmtData, StmtId, StructId, TraitId,
    TypeData, TypeRefId,
};

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub items: ItemBuffer,
    pub exports: ExportBuffer,
    pub module_init: Option<FunctionId>,
    pub functions: FunctionBuffer,
    pub methods: MethodBuffer,
    pub consts: ConstBuffer,
    pub statics: StaticBuffer,
    pub modules: ModuleDeclBuffer,
    pub structs: StructBuffer,
    pub enums: EnumBuffer,
    pub traits: TraitBuffer,
    pub impls: ImplBuffer,
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
    Module(ModuleId),
    Struct(StructId),
    Enum(EnumId),
    Trait(TraitId),
    Impl(ImplId),
}

pub type ItemBuffer = Vec<Item>;
