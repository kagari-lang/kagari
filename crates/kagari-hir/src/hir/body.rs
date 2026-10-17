//! Owner-tagged node vectors and arena-checked access to a module's lowered bodies.

use crate::hir::{
    expr::ExprData,
    ids::{BlockId, ExprId, HirArenaId, HirOwner, PatternId, PlaceId, StmtId, TypeRefId},
    pattern::PatternData,
    place::PlaceData,
    stmt::{BlockData, StmtData},
    ty::TypeData,
};

/// Node storage shared by all declarations and function/constant bodies in a module.
///
/// The internal `Lowerer` appends a source-map entry and a node row at
/// the same index. Each row carries its [`HirOwner`]; this is not a separate arena
/// per function. Cloning preserves the arena identity and row order.
///
/// # Storage and lookup
///
/// For `fn add(x: i32) -> i32 { x + 1 }`, with illustrative indices:
///
/// ```text
/// Module.functions[f] -> Function { body: Some(b), ... }
/// Body.blocks[b.index] -> (Body(Function(f)), BlockData { tail_expr: Some(e2), ... })
/// Body.exprs
/// +-- e0.index -> (Body(Function(f)), Name { name: "x", ... })
/// +-- e1.index -> (Body(Function(f)), Literal(...))
/// `-- e2.index -> (Body(Function(f)), Binary { lhs: e0, op: Add, rhs: e1 })
///
/// e2 = ExprId { arena, owner: Body(Function(f)), index }
/// expr(e2): assert arena -> exprs[index] -> assert owner -> borrow ExprData
/// ```
///
/// Name resolution, semantic types and source spans are separate tables keyed by
/// these IDs. Parameters are held by functions and local bindings by statements,
/// patterns or closure parameters; neither has its own vector in `Body`.
///
/// For the example above, `blocks` contains the function's block, `exprs` contains
/// both operand occurrences and the addition, `types` contains the parameter and
/// result i32 syntax, and `stmts`/`places`/`patterns` are empty. Adding `val y = x;`
/// creates a statement row; `y = x;` creates a place row; `match x { n => n }`
/// creates pattern rows. Every vector row stores `(allocation_owner, payload)`.
/// Parameters/locals still live inline in their declaration/statement/pattern/
/// closure records; their IDs address source-map slots and semantic binding maps.
///
/// `arena` is synthesized once for the lowering and shared with SourceMap, not
/// written in source. `HirOwner::Declaration` marks nodes outside function/constant
/// construction; `Body(Function(f))` and `Body(Const(c))` mark those allocation
/// contexts. `index` addresses the whole corresponding vector, not a per-owner
/// list. Body::expressions reconstructs `(ExprId, &ExprData)` from these rows;
/// payloads therefore need no self ID. New lowering creates a new arena; retaining
/// a snapshot retains its matching rows and IDs rather than making IDs persistent.
///
/// # Panics
///
/// Node accessors panic for a foreign arena, wrong owner or out-of-range index.
/// These indicate mixed lowerings or invalid internal references, not user syntax
/// errors. Retain the matching [`crate::lower::LoweredModule`] when retaining IDs.
#[derive(Debug, Clone, Default)]
pub struct Body {
    /// Identity shared with the source map that allocated these rows.
    pub(crate) arena: HirArenaId,
    /// Block rows paired with their allocation owners.
    pub(crate) blocks: Vec<(HirOwner, BlockData)>,
    /// Statement rows paired with their allocation owners.
    pub(crate) stmts: Vec<(HirOwner, StmtData)>,
    /// Expression rows paired with their allocation owners.
    pub(crate) exprs: Vec<(HirOwner, ExprData)>,
    /// Assignment-place rows paired with their allocation owners.
    pub(crate) places: Vec<(HirOwner, PlaceData)>,
    /// Pattern rows paired with their allocation owners.
    pub(crate) patterns: Vec<(HirOwner, PatternData)>,
    /// Type-syntax rows; resolved types are stored in the type checker.
    pub(crate) types: Vec<(HirOwner, TypeData)>,
}

impl Body {
    /// Returns the identity of this lowering, not a function or source revision.
    pub fn arena(&self) -> HirArenaId {
        self.arena
    }

    /// Visits expression rows in allocation order, reconstructing their complete IDs.
    pub fn expressions(&self) -> impl Iterator<Item = (ExprId, &ExprData)> {
        self.exprs
            .iter()
            .enumerate()
            .map(|(index, (owner, expr))| (ExprId::new(self.arena, *owner, index), expr))
    }

    /// Visits place rows in allocation order, reconstructing their complete IDs.
    pub fn places(&self) -> impl Iterator<Item = (PlaceId, &PlaceData)> {
        self.places
            .iter()
            .enumerate()
            .map(|(index, (owner, place))| (PlaceId::new(self.arena, *owner, index), place))
    }

    /// Visits statement rows in allocation order, reconstructing their complete IDs.
    pub fn statements(&self) -> impl Iterator<Item = (StmtId, &StmtData)> {
        self.stmts
            .iter()
            .enumerate()
            .map(|(index, (owner, stmt))| (StmtId::new(self.arena, *owner, index), stmt))
    }

    /// Visits block rows in allocation order, reconstructing their complete IDs.
    pub fn blocks(&self) -> impl Iterator<Item = (BlockId, &BlockData)> {
        self.blocks
            .iter()
            .enumerate()
            .map(|(index, (owner, block))| (BlockId::new(self.arena, *owner, index), block))
    }

    /// Borrows the block row addressed by `id`; see the [storage checks](Self#panics).
    pub fn block(&self, id: BlockId) -> &BlockData {
        assert_eq!(id.arena(), self.arena, "foreign HIR block");
        let (owner, node) = &self.blocks[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    /// Borrows the statement row addressed by `id`; see the [storage checks](Self#panics).
    pub fn stmt(&self, id: StmtId) -> &StmtData {
        assert_eq!(id.arena(), self.arena, "foreign HIR statement");
        let (owner, node) = &self.stmts[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    /// Borrows the expression row addressed by `id`; see the [storage checks](Self#panics).
    pub fn expr(&self, id: ExprId) -> &ExprData {
        assert_eq!(id.arena(), self.arena, "foreign HIR expression");
        let (owner, node) = &self.exprs[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    /// Borrows the assignment place row addressed by `id`; see the [storage checks](Self#panics).
    pub fn place(&self, id: PlaceId) -> &PlaceData {
        assert_eq!(id.arena(), self.arena, "foreign HIR place");
        let (owner, node) = &self.places[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    /// Borrows the pattern row addressed by `id`; see the [storage checks](Self#panics).
    pub fn pattern(&self, id: PatternId) -> &PatternData {
        assert_eq!(id.arena(), self.arena, "foreign HIR pattern");
        let (owner, node) = &self.patterns[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    /// Borrows the type-syntax row addressed by `id`; see the [storage checks](Self#panics).
    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        assert_eq!(id.arena(), self.arena, "foreign HIR type reference");
        let (owner, node) = &self.types[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }
}

/// Contiguous statement payloads, without the owner pairs used by `Body`.
pub type StmtDataBuffer = Vec<StmtData>;
/// Contiguous assignment-place payloads, without owner pairs.
pub type PlaceDataBuffer = Vec<PlaceData>;
/// Contiguous type-syntax payloads, without owner pairs.
pub type TypeDataBuffer = Vec<TypeData>;
