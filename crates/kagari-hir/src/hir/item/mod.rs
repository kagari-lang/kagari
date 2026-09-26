mod adt;
mod behavior;
mod function;
mod module;
mod storage;

pub use adt::{Enum, EnumBuffer, Field, FieldBuffer, Struct, StructBuffer, Variant, VariantBuffer};
pub use behavior::{
    AssociatedType, GenericParam, GenericParamBuffer, Impl, ImplBuffer, ImplMethod,
    ImplMethodBuffer, Method, MethodBuffer, MethodOwner, ReceiverKind, TraitBound,
    TraitBoundBuffer, TraitBuffer, TraitDef, TraitMethod, TraitMethodBuffer, TraitRef,
    TraitRefBuffer,
};
pub use function::{Function, FunctionBuffer, FunctionKind, Param, ParamBuffer};
pub use module::{Import, ImportBuffer, ModuleDecl, ModuleDeclBuffer};
pub use storage::{ConstBuffer, ConstItem, Export, ExportBuffer, ExportItem, Visibility};

use crate::hir::{
    BlockData, BlockId, Body, ConstId, EnumId, ExprData, ExprId, FunctionId, ImplId, ModuleId,
    PatternData, PatternId, PlaceData, PlaceId, StmtData, StmtId, StructId, TraitId, TypeData,
    TypeRefId,
};

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

    pub fn variant(&self, id: crate::hir::VariantId) -> &Variant {
        assert_eq!(id.arena(), self.body.arena(), "foreign HIR variant");
        &self.enums[id.owner().index()].variants[id.slot()]
    }

    pub fn field(&self, id: crate::hir::FieldId) -> &Field {
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

    pub fn pattern_is_irrefutable(&self, id: PatternId) -> bool {
        match &self.pattern(id).kind {
            crate::hir::PatternKind::Wildcard | crate::hir::PatternKind::Name { .. } => true,
            crate::hir::PatternKind::Tuple(elements) => elements
                .iter()
                .all(|element| self.pattern_is_irrefutable(*element)),
            crate::hir::PatternKind::Struct { fields, .. } => fields
                .iter()
                .all(|field| self.pattern_is_irrefutable(field.pattern)),
            crate::hir::PatternKind::Or(alternatives) => alternatives
                .iter()
                .any(|alternative| self.pattern_is_irrefutable(*alternative)),
            crate::hir::PatternKind::Range { .. }
            | crate::hir::PatternKind::Literal(_)
            | crate::hir::PatternKind::EnumVariant { .. } => false,
        }
    }

    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        self.body.type_ref(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Function(FunctionId),
    Const(ConstId),
    Module(ModuleId),
    Struct(StructId),
    Enum(EnumId),
    Trait(TraitId),
    Impl(ImplId),
}

pub type ItemBuffer = Vec<Item>;
