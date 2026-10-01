use crate::hir::{
    body::Body,
    expr::ExprData,
    ids::{
        BlockId, ConstId, EnumId, ExprId, FieldId, FunctionId, ImplId, ModuleId, OpaqueTypeId,
        PatternId, PlaceId, StmtId, StructId, TraitId, TypeRefId, VariantId,
    },
    item::{
        adt::{EnumBuffer, Field, OpaqueType, StructBuffer, Variant},
        behavior::{ImplBuffer, MethodBuffer, TraitBuffer},
        function::FunctionBuffer,
        module::{ImportBuffer, ModuleDeclBuffer},
        storage::{ConstBuffer, ConstItem, ExportBuffer},
    },
    pattern::PatternData,
    place::PlaceData,
    stmt::{BlockData, StmtData},
    ty::TypeData,
};
pub mod adt;
pub mod behavior;
pub mod function;
pub mod module;
pub mod storage;

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub items: ItemBuffer,
    pub exports: ExportBuffer,
    pub functions: FunctionBuffer,
    pub methods: MethodBuffer,
    pub consts: ConstBuffer,
    pub modules: ModuleDeclBuffer,
    pub imports: ImportBuffer,
    pub structs: StructBuffer,
    pub opaque_types: Vec<OpaqueType>,
    pub enums: EnumBuffer,
    pub traits: TraitBuffer,
    pub impls: ImplBuffer,
    pub body: Body,
}

impl Module {
    /// Const IDs address declaration slots within this module's lowering.
    pub fn constant(&self, id: ConstId) -> &ConstItem {
        let item = &self.consts[id.index()];
        assert_eq!(item.id, id, "constant declaration slot mismatch");
        item
    }

    pub fn variant(&self, id: VariantId) -> &Variant {
        assert_eq!(id.arena(), self.body.arena(), "foreign HIR variant");
        &self.enums[id.owner().index()].variants[id.slot()]
    }

    pub fn field(&self, id: FieldId) -> &Field {
        assert_eq!(id.arena(), self.body.arena(), "foreign HIR field");
        &self.structs[id.owner().index()].fields[id.slot()]
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    OpaqueType(OpaqueTypeId),
    Function(FunctionId),
    Const(ConstId),
    Module(ModuleId),
    Struct(StructId),
    Enum(EnumId),
    Trait(TraitId),
    Impl(ImplId),
}

pub type ItemBuffer = Vec<Item>;
