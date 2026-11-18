//! Module declaration collections and handles into their shared HIR storage.

use crate::hir::{
    attribute::{AssociatedOwner, AttributeFact, AttributeTarget},
    expr::ExprData,
    ids::{
        BlockId, BodyOwner, ConstId, EnumId, ExprId, FieldId, FieldOwner, FunctionId,
        GenericParamId, HirOwner, ImplId, ModuleId, NativeTypeId, ParamId, PatternId, PlaceId,
        StaticId, StmtId, StructId, TraitId, TraitMethodId, TypeAliasId, TypeRefId, VariantId,
    },
    item::{
        adt::{Enum, Field, NativeTypeDeclaration, Struct, Variant},
        alias::TypeAlias,
        behavior::{GenericParam, Impl, TraitDef, TraitMethod},
        function::{Function, FunctionKind, Param},
        module::{Import, ModuleDecl, UseDecl},
        storage::{ConstItem, StaticItem},
    },
    nodes::HirNodes,
    origin::NodeOrigin,
    pattern::PatternData,
    place::PlaceData,
    stmt::{BlockData, StmtData},
    ty::TypeData,
};
use kagari_common::span::Span;

pub mod adt;
pub mod alias;
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
///     imports: [Import { segments: ["pkg", "math", "sum"], kind: Named { alias: None }, ... }],
///     modules: [], native_types: [], enums: [], traits: [], impls: [],
///     nodes: HirNodes { types, exprs, blocks, ... },
/// }
/// f -> functions[f.index()]; s -> structs[s.index()]; c -> constant(c)
/// main_function.implementation -> Script(BlockId) -> nodes.block -> statements/tail ExprId
/// ```
///
/// Rows/indices above are illustrative and abbreviated where marked. `items`
/// contains top-level declaration handles in source order, excluding use leaves.
/// Each separate collection stores payloads of one declaration kind: enum/trait/
/// impl/native type/module syntax populates enums/traits/impls/native_types/modules.
/// Trait/impl methods also allocate function rows and associated constant defaults
/// allocate constant rows, but do not add top-level Item entries for those members.
///
/// `nodes` owns declaration type syntax and all function/constant nodes together;
/// it is not one HirNodes per function. Child module rows are headers, with their
/// contents analyzed in separate modules. LoweredModule pairs this record with
/// its source and input kind. Resolution/typing consume it and publish separate facts.
/// allocation does not establish source validity. Resolved scope/export candidates
/// belong to the import graph, including public glob expansion; there is no
/// additional export collection here.
#[derive(Debug, Clone, Default)]
pub struct Module {
    /// Uninterpreted attributes in authored source order, attached to typed identities.
    pub attributes: Vec<AttributeFact>,
    /// Authored use declarations indexed by `UseId`, including recovery with no leaves.
    pub uses: Vec<UseDecl>,
    /// Top-level declaration handles in source order; import leaves are stored separately.
    pub items: Vec<Item>,
    /// Function declarations indexed by `FunctionId`, including trait/impl methods.
    pub functions: Vec<Function>,
    /// Constant declarations indexed by `ConstId`, including associated initializers.
    pub consts: Vec<ConstItem>,
    /// Runtime static declarations indexed by `StaticId`.
    pub statics: Vec<StaticItem>,
    /// Child module headers indexed by `ModuleId`; child analysis is separate.
    pub modules: Vec<ModuleDecl>,
    /// Flattened named/glob use leaves, indexed by import slot.
    pub imports: Vec<Import>,
    /// Struct declarations indexed by `StructId`.
    pub structs: Vec<Struct>,
    /// Native-backed type declarations indexed by `NativeTypeId`.
    pub native_types: Vec<NativeTypeDeclaration>,
    /// Transparent script type declarations indexed by `TypeAliasId`.
    pub type_aliases: Vec<TypeAlias>,
    /// Enum declarations indexed by `EnumId`; variants are nested within them.
    pub enums: Vec<Enum>,
    /// Trait declarations indexed by `TraitId`.
    pub traits: Vec<TraitDef>,
    /// Implementation blocks indexed by `ImplId`.
    pub impls: Vec<Impl>,
    /// Shared node storage for declaration types and all body owners.
    pub nodes: HirNodes,
    /// Authored generic binder records shared by effective declaration lists.
    pub(crate) generic_params: Vec<GenericParam>,
}

impl Module {
    /// Borrows the authored binder for this module-local ID.
    ///
    /// Panics if the slot is out of bounds. The ID belongs to the matching lowering.
    pub fn generic_parameter(&self, id: GenericParamId) -> &GenericParam {
        &self.generic_params[id.index()]
    }

    /// Borrows a method from its trait's inline list.
    ///
    /// Panics for a foreign lowering or invalid trait/method slot.
    pub fn trait_method(&self, id: TraitMethodId) -> &TraitMethod {
        assert_eq!(id.arena(), self.nodes.arena(), "foreign HIR trait method");
        let method = &self.traits[id.owner().index()].methods[id.slot()];
        assert_eq!(method.id, id, "trait method slot mismatch");
        method
    }

    /// Returns the full origin and optional written name range of an item slot.
    ///
    /// IDs belong to the matching module. An absent slot returns `None`.
    pub fn item_source(&self, item: Item) -> Option<(NodeOrigin, Option<Span>)> {
        macro_rules! source {
            ($records:ident, $id:ident) => {{
                let item = self.$records.get($id.index())?;
                (item.id == $id).then_some((item.origin, item.name_span))
            }};
        }

        match item {
            Item::Function(id) => source!(functions, id),
            Item::Const(id) => source!(consts, id),
            Item::Static(id) => source!(statics, id),
            Item::Module(id) => source!(modules, id),
            Item::Struct(id) => source!(structs, id),
            Item::NativeType(id) => source!(native_types, id),
            Item::TypeAlias(id) => source!(type_aliases, id),
            Item::Enum(id) => source!(enums, id),
            Item::Trait(id) => source!(traits, id),
            Item::Impl(id) => {
                let item = self.impls.get(id.index())?;
                (item.id == id).then_some((item.origin, None))
            }
        }
    }

    /// Returns the full origin of a typed attribute target in this module.
    ///
    /// Panics for an invalid target slot or foreign qualified handle.
    pub fn attribute_target_origin(&self, target: AttributeTarget) -> NodeOrigin {
        match target {
            AttributeTarget::Item(item) => self.item_source(item).expect("attribute item slot").0,
            AttributeTarget::Use(id) => self.uses[id.index()].origin,
            AttributeTarget::Field(id) => self.field(id).origin,
            AttributeTarget::Variant(id) => self.variant(id).origin,
            AttributeTarget::AssociatedType { owner, index } => match owner {
                AssociatedOwner::Trait(id) => {
                    self.traits[id.index()].associated_types[index].origin
                }
                AssociatedOwner::Impl(id) => self.impls[id.index()].associated_types[index].origin,
            },
            AttributeTarget::AssociatedConst { owner, index } => match owner {
                AssociatedOwner::Trait(id) => {
                    self.traits[id.index()].associated_consts[index].origin
                }
                AssociatedOwner::Impl(id) => self.impls[id.index()].associated_consts[index].origin,
            },
        }
    }

    pub(crate) fn static_initializer_for_function(&self, mut id: FunctionId) -> Option<StaticId> {
        while let FunctionKind::Local { block } = self.functions[id.index()].kind {
            match block.owner() {
                HirOwner::Body(BodyOwner::Static(id)) => return Some(id),
                HirOwner::Body(BodyOwner::Function(parent)) => id = parent,
                _ => return None,
            }
        }
        None
    }

    /// Borrows a runtime static declaration in this module's lowering.
    ///
    /// # Panics
    ///
    /// Panics for an out-of-range or mismatched declaration slot.
    pub fn static_item(&self, id: StaticId) -> &StaticItem {
        let item = &self.statics[id.index()];
        assert_eq!(item.id, id, "static declaration slot mismatch");
        item
    }

    /// Const IDs address declaration slots within this module's lowering.
    pub fn constant(&self, id: ConstId) -> &ConstItem {
        let item = &self.consts[id.index()];
        assert_eq!(item.id, id, "constant declaration slot mismatch");
        item
    }

    /// Borrows the authoritative record at the ID's owning function and parameter position.
    ///
    /// # Panics
    ///
    /// Panics for a foreign lowering, a non-function owner, an absent function/parameter
    /// slot, or a stored identity that does not match the requested ID.
    pub fn parameter(&self, id: ParamId) -> &Param {
        assert_eq!(id.arena(), self.nodes.arena(), "foreign HIR parameter");
        let HirOwner::Body(BodyOwner::Function(owner)) = id.owner() else {
            panic!("HIR parameter requires a function owner");
        };
        let function = &self.functions[owner.index()];
        assert_eq!(function.id, owner, "function declaration slot mismatch");
        let parameter = &function.params[id.index()];
        assert_eq!(parameter.id, id, "parameter slot mismatch");
        parameter
    }

    /// Borrows `enums[id.owner().index()].variants[id.slot()]`.
    ///
    /// # Panics
    ///
    /// Panics for a foreign arena or out-of-range owner/slot.
    pub fn variant(&self, id: VariantId) -> &Variant {
        assert_eq!(id.arena(), self.nodes.arena(), "foreign HIR variant");
        &self.enums[id.owner().index()].variants[id.slot()]
    }

    /// Borrows a field from its owning struct or Native declaration.
    ///
    /// # Panics
    ///
    /// Panics for a foreign arena or out-of-range owner/slot.
    pub fn field(&self, id: FieldId) -> &Field {
        assert_eq!(id.arena(), self.nodes.arena(), "foreign HIR field");
        let fields = match id.owner() {
            FieldOwner::Struct(owner) => {
                let declaration = &self.structs[owner.index()];
                assert_eq!(declaration.id, owner, "struct declaration slot mismatch");
                &declaration.fields
            }
            FieldOwner::NativeType(owner) => {
                let declaration = &self.native_types[owner.index()];
                assert_eq!(declaration.id, owner, "native declaration slot mismatch");
                &declaration.fields
            }
        };
        let field = &fields[id.slot()];
        assert_eq!(field.id, id, "field slot mismatch");
        field
    }

    /// Borrows node data through [`HirNodes::block`], including its arena/owner checks.
    pub fn block(&self, id: BlockId) -> &BlockData {
        self.nodes.block(id)
    }

    /// Borrows node data through [`HirNodes::stmt`], including its arena/owner checks.
    pub fn stmt(&self, id: StmtId) -> &StmtData {
        self.nodes.stmt(id)
    }

    /// Borrows node data through [`HirNodes::expr`], including its arena/owner checks.
    pub fn expr(&self, id: ExprId) -> &ExprData {
        self.nodes.expr(id)
    }

    /// Borrows node data through [`HirNodes::place`], including its arena/owner checks.
    pub fn place(&self, id: PlaceId) -> &PlaceData {
        self.nodes.place(id)
    }

    /// Borrows node data through [`HirNodes::pattern`], including its arena/owner checks.
    pub fn pattern(&self, id: PatternId) -> &PatternData {
        self.nodes.pattern(id)
    }

    /// Borrows node data through [`HirNodes::type_ref`], including its arena/owner checks.
    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        self.nodes.type_ref(id)
    }
}

/// A top-level declaration link selecting a payload collection in Module.
///
/// | Source declaration | Item | Payload lookup |
/// | --- | --- | --- |
/// | `native type Handle;` (Native interface) | `NativeType(o)` | `native_types[o.index()]` |
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
/// standalone entries in this vector. Provenance queries use these links for declaration
/// and name sites; later collection builds semantic declaration identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    /// An entry in [`Module::type_aliases`], with no nominal/runtime identity.
    TypeAlias(TypeAliasId),
    /// An entry in [`Module::native_types`], indexed by the enclosed ID.
    NativeType(NativeTypeId),
    /// An entry in [`Module::functions`], indexed by the enclosed ID.
    Function(FunctionId),
    /// An entry in [`Module::consts`], indexed by the enclosed ID.
    Const(ConstId),
    /// An entry in [`Module::statics`], indexed by the enclosed ID.
    Static(StaticId),
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
