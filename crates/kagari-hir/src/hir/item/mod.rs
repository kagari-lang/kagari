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

/// Declaration collections and shared node tables for one logical source module.
///
/// ```text
/// use pkg::math::sum;
/// const LIMIT: i32 = 10;
/// struct Point { val x: i32 }
/// fn main() -> i32 { sum(LIMIT) }
///
/// Module {
///     items: [Const(c), Struct(s), Function(f)],
///     functions: [main_function], consts: [limit_constant], structs: [point_struct],
///     imports: [Import { path: "pkg::math::sum", kind: Named { alias: None }, ... }],
///     methods: [], modules: [], opaque_types: [], enums: [], traits: [], impls: [],
///     body: Body { types, exprs, blocks, ... },
/// }
/// f -> functions[f.index()]; s -> structs[s.index()]; c -> constant(c)
/// main_function.body -> BlockId -> body.block -> statements/tail ExprId
/// ```
///
/// Rows/indices above are illustrative and abbreviated where marked. `items`
/// contains top-level declaration handles in source order, excluding use leaves.
/// Each separate collection stores payloads of one declaration kind: enum/trait/
/// impl/type/module syntax populates enums/traits/impls/opaque_types/modules.
/// Trait/impl methods also allocate function rows and associated constant defaults
/// allocate constant rows, but do not add top-level Item entries for those members.
/// Current `methods` is an unpopulated alternative registry, not active method storage.
///
/// `body` owns declaration type syntax and all function/constant nodes together;
/// it is not one Body per function. Child module rows are headers, with their
/// contents analyzed in separate modules. LoweredModule pairs this record with
/// source and SourceMap. Resolution/typing consume it and publish separate facts;
/// allocation does not establish source validity. Resolved scope/export candidates
/// belong to the import graph, including public glob expansion; there is no
/// additional export collection here.
#[derive(Debug, Clone, Default)]
pub struct Module {
    /// Top-level declaration handles in source order; import leaves are stored separately.
    pub items: ItemBuffer,
    /// Function declarations indexed by `FunctionId`, including trait/impl methods.
    pub functions: FunctionBuffer,
    /// Unpopulated MethodId registry; active methods link from trait/impl records to functions.
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

/// A top-level declaration link selecting a payload collection in Module.
///
/// | Source declaration | Item | Payload lookup |
/// | --- | --- | --- |
/// | `type Handle;` (installed native surface) | `OpaqueType(o)` | `opaque_types[o.index()]` |
/// | `fn run() {}` | `Function(f)` | `functions[f.index()]` |
/// | `const N: i32 = 1;` | `Const(c)` | `constant(c)` |
/// | `mod child;` / `mod child { ... }` | `Module(m)` | `modules[m.index()]`, header only |
/// | `struct S {}` | `Struct(s)` | `structs[s.index()]` |
/// | `enum E { A }` | `Enum(e)` | `enums[e.index()]` |
/// | `trait R { ... }` | `Trait(t)` | `traits[t.index()]` |
/// | `impl R for S { ... }` | `Impl(i)` | `impls[i.index()]` |
///
/// The enclosed IDs are lowering-local, not checked DefinitionIds. Item carries
/// no declaration body itself; Module.items retains source declaration order
/// across kinds. Imports are separate leaves, and trait/impl members are not
/// standalone entries in this vector. SourceMap uses these links for declaration
/// and name sites; later collection builds semantic declaration identities.
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
