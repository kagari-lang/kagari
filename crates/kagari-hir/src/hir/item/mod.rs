//! Module declaration collections and handles into their shared HIR storage.

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
        storage::{ConstBuffer, ConstItem},
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

/// Declarations and node storage produced for one logical source module.
///
/// [`crate::lower::LoweredModule`] pairs this value with its source and source map.
/// Plain declaration IDs index the matching collection here; node IDs enter
/// [`Body`]. A child module declaration is a header, not an embedded child `Module`.
///
/// ```text
/// Module
/// +-- items: [Function(f), Struct(s), ...]   // declaration order; no use leaves
/// +-- functions[f.index()] -> Function { params, body: Some(block_id), ... }
/// +-- structs[s.index()] -> Struct { fields: [Field, ...], ... }
/// +-- imports: [Import, ...]               // one entry per use-tree leaf
/// `-- body: Body                           // blocks/expressions/types/... by ID
/// ```
///
/// Resolved scope/export candidates belong to the import graph. In particular,
/// public glob imports are expanded there. This value alone proves neither resolution
/// nor typing.
#[derive(Debug, Clone, Default)]
pub struct Module {
    /// Top-level declaration handles in source order; import leaves are stored separately.
    pub items: ItemBuffer,
    /// Function declarations indexed by `FunctionId`, including trait/impl methods.
    pub functions: FunctionBuffer,
    /// Method records indexed by `MethodId`; their functions live in `functions`.
    pub methods: MethodBuffer,
    /// Constant declarations indexed by `ConstId`, including associated initializers.
    pub consts: ConstBuffer,
    /// Child module headers indexed by `ModuleId`; child analysis is separate.
    pub modules: ModuleDeclBuffer,
    /// Flattened named/glob use leaves, indexed by import slot.
    pub imports: ImportBuffer,
    /// Struct declarations indexed by `StructId`.
    pub structs: StructBuffer,
    /// Native-backed type declarations indexed by `OpaqueTypeId`.
    pub opaque_types: Vec<OpaqueType>,
    /// Enum declarations indexed by `EnumId`; variants are nested within them.
    pub enums: EnumBuffer,
    /// Trait declarations indexed by `TraitId`.
    pub traits: TraitBuffer,
    /// Implementation blocks indexed by `ImplId`.
    pub impls: ImplBuffer,
    /// Shared node storage for declaration types and all body owners.
    pub body: Body,
}

impl Module {
    /// Const IDs address declaration slots within this module's lowering.
    pub fn constant(&self, id: ConstId) -> &ConstItem {
        let item = &self.consts[id.index()];
        assert_eq!(item.id, id, "constant declaration slot mismatch");
        item
    }

    /// Borrows `enums[id.owner().index()].variants[id.slot()]`.
    ///
    /// # Panics
    ///
    /// Panics for a foreign arena or out-of-range owner/slot.
    pub fn variant(&self, id: VariantId) -> &Variant {
        assert_eq!(id.arena(), self.body.arena(), "foreign HIR variant");
        &self.enums[id.owner().index()].variants[id.slot()]
    }

    /// Borrows `structs[id.owner().index()].fields[id.slot()]`.
    ///
    /// # Panics
    ///
    /// Panics for a foreign arena or out-of-range owner/slot.
    pub fn field(&self, id: FieldId) -> &Field {
        assert_eq!(id.arena(), self.body.arena(), "foreign HIR field");
        &self.structs[id.owner().index()].fields[id.slot()]
    }

    /// Borrows node data through [`Body::block`], including its arena/owner checks.
    pub fn block(&self, id: BlockId) -> &BlockData {
        self.body.block(id)
    }

    /// Borrows node data through [`Body::stmt`], including its arena/owner checks.
    pub fn stmt(&self, id: StmtId) -> &StmtData {
        self.body.stmt(id)
    }

    /// Borrows node data through [`Body::expr`], including its arena/owner checks.
    pub fn expr(&self, id: ExprId) -> &ExprData {
        self.body.expr(id)
    }

    /// Borrows node data through [`Body::place`], including its arena/owner checks.
    pub fn place(&self, id: PlaceId) -> &PlaceData {
        self.body.place(id)
    }

    /// Borrows node data through [`Body::pattern`], including its arena/owner checks.
    pub fn pattern(&self, id: PatternId) -> &PatternData {
        self.body.pattern(id)
    }

    /// Borrows node data through [`Body::type_ref`], including its arena/owner checks.
    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        self.body.type_ref(id)
    }
}

/// A top-level declaration handle selecting a collection in [`Module`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    /// An entry in [`Module::opaque_types`], indexed by the enclosed ID.
    OpaqueType(OpaqueTypeId),
    /// An entry in [`Module::functions`], indexed by the enclosed ID.
    Function(FunctionId),
    /// An entry in [`Module::consts`], indexed by the enclosed ID.
    Const(ConstId),
    /// An entry in [`Module::modules`], indexed by the enclosed ID.
    Module(ModuleId),
    /// An entry in [`Module::structs`], indexed by the enclosed ID.
    Struct(StructId),
    /// An entry in [`Module::enums`], indexed by the enclosed ID.
    Enum(EnumId),
    /// An entry in [`Module::traits`], indexed by the enclosed ID.
    Trait(TraitId),
    /// An entry in [`Module::impls`], indexed by the enclosed ID.
    Impl(ImplId),
}

/// Source-ordered declaration handles, excluding import leaves.
pub type ItemBuffer = Vec<Item>;
