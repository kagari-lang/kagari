//! Owner-tagged node vectors and lowering-checked access to a module's lowered bodies.

use crate::hir::{
    expr::ExprData,
    ids::{BlockId, ExprId, HirOwner, LocalId, LoweringId, PatternId, PlaceId, StmtId, TypeRefId},
    origin::NodeOrigin,
    pattern::{PatternData, PatternKind},
    place::PlaceData,
    stmt::{BlockData, StmtData},
    ty::TypeData,
};

/// A name declared by a pattern or closure parameter.
#[derive(Debug, Clone)]
pub struct Local {
    /// Source range or structured declaration attribution.
    pub origin: NodeOrigin,
    /// Written name, or the lowering's recovery name.
    pub name: String,
}

/// Node storage shared by all declarations and function/constant bodies in a module.
///
/// The internal `Lowerer` allocates IDs from the category row index. Each row
/// pairs an [`HirOwner`] with a payload that carries its origin. Cloning preserves
/// the lowering identity and row order.
///
/// # Storage and lookup
///
/// For `fn add(x: i32) -> i32 { x + 1 }`, with illustrative indices:
///
/// ```text
/// Module.functions[f] -> Function { implementation: Script(b), ... }
/// HirNodes.blocks[b.index] -> (Body(Function(f)), BlockData { tail_expr: Some(e2), ... })
/// HirNodes.exprs
/// +-- e0.index -> (Body(Function(f)), Name { name: "x", ... })
/// +-- e1.index -> (Body(Function(f)), Literal(...))
/// `-- e2.index -> (Body(Function(f)), Binary { lhs: e0, op: Add, rhs: e1 })
///
/// e2 = ExprId { arena, owner: Body(Function(f)), index }
/// expr(e2): assert arena -> exprs[index] -> assert owner -> borrow ExprData
/// ```
///
/// Name resolution and semantic types are separate tables keyed by these IDs. Functions own their parameters. Pattern leaves and closure
/// parameters link to the provisional name records in `locals`.
///
/// For the example above, `blocks` contains the function's block, `exprs` contains
/// both operand occurrences and the addition, `types` contains the parameter and
/// result i32 syntax, and `stmts`/`places`/`patterns` are empty. Adding `val y = x;`
/// creates statement and pattern rows; `y = x;` creates a place row;
/// `match x { n => n }` creates pattern rows. Every vector row stores
/// `(allocation_owner, payload)`. ParamId indexes its owning function's
/// parameter list. LocalId indexes the module-wide local name records and keys
/// semantic binding maps.
///
/// The `arena` field stores the lowering identity rather than a value
/// written in source. `HirOwner::Declaration` marks nodes outside function/constant
/// construction; `Body(Function(f))` and `Body(Const(c))` mark those allocation
/// contexts. `index` addresses the whole corresponding vector, not a per-owner
/// list. HirNodes::expressions reconstructs `(ExprId, &ExprData)` from these rows;
/// payloads therefore need no self ID. New lowering creates a new identity; retaining
/// a snapshot retains its matching rows and IDs rather than making IDs persistent.
///
/// # Panics
///
/// Node accessors panic for a foreign lowering, wrong owner or out-of-range index.
/// These indicate mixed lowerings or invalid internal references, not user syntax
/// errors. Retain the matching [`crate::lower::LoweredModule`] when retaining IDs.
#[derive(Debug, Clone, Default)]
pub struct HirNodes {
    /// Identity of this lowering, shared by its qualified IDs.
    pub(crate) arena: LoweringId,
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
    /// Provisional local names, in lowering order.
    pub(crate) locals: Vec<(HirOwner, Local)>,
}

impl HirNodes {
    /// Returns the identity of this lowering, not a function or source revision.
    pub fn arena(&self) -> LoweringId {
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

    /// Visits type syntax in allocation order with matching lowering/owner IDs.
    pub fn types(&self) -> impl Iterator<Item = (TypeRefId, &TypeData)> {
        self.types
            .iter()
            .enumerate()
            .map(|(index, (owner, ty))| (TypeRefId::new(self.arena, *owner, index), ty))
    }

    /// Visits patterns in allocation order with matching lowering/owner IDs.
    pub fn patterns(&self) -> impl Iterator<Item = (PatternId, &PatternData)> {
        self.patterns
            .iter()
            .enumerate()
            .map(|(index, (owner, pattern))| (PatternId::new(self.arena, *owner, index), pattern))
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

    pub(crate) fn origins(&self) -> impl Iterator<Item = &NodeOrigin> {
        self.blocks
            .iter()
            .map(|(_, node)| &node.origin)
            .chain(self.stmts.iter().map(|(_, node)| &node.origin))
            .chain(self.exprs.iter().map(|(_, node)| &node.origin))
            .chain(self.places.iter().map(|(_, node)| &node.origin))
            .chain(self.patterns.iter().map(|(_, node)| &node.origin))
            .chain(self.types.iter().map(|(_, node)| &node.origin))
            .chain(self.locals.iter().map(|(_, node)| &node.origin))
    }

    /// Borrows a local name in the matching lowering and owner.
    ///
    /// See the [storage checks](Self#panics).
    pub fn local(&self, id: LocalId) -> &Local {
        assert_eq!(id.arena(), self.arena, "foreign HIR local");
        let (owner, node) = &self.locals[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }

    /// Collects a declaration pattern's provisional name leaves in source order.
    ///
    /// Traversal is depth-first through tuples, struct fields (not declaration
    /// slot order), and constructor payloads. Only the first `Or` alternative
    /// supplies canonical bindings, matching lexical publication. For example,
    /// `val (x, Point { y: z, .. }) = value;` yields `[(x_local, "x"),
    /// (z_local, "z")]`. Wildcards, literals and ranges supply no binding.
    ///
    /// This is structural traversal, not resolution or validation: names borrow
    /// this body, duplicates/recovery names are retained, and even a refutable
    /// constructor's payload leaves are returned. Use it only for declaration
    /// patterns, where bare names always bind. Match/for/condition names can
    /// instead denote variants and require their resolved-name policy.
    ///
    /// Panics for foreign or invalid pattern IDs; see [storage checks](Self#panics).
    pub fn pattern_bindings(&self, pattern: PatternId) -> Vec<(LocalId, &str)> {
        let mut bindings = Vec::new();
        let mut work = vec![pattern];
        while let Some(pattern) = work.pop() {
            match &self.pattern(pattern).kind {
                PatternKind::Name { local } => {
                    bindings.push((*local, self.local(*local).name.as_str()))
                }
                PatternKind::Or(alternatives) => work.extend(alternatives.first().copied()),
                PatternKind::Tuple(elements)
                | PatternKind::EnumVariant {
                    fields: elements, ..
                } => {
                    work.extend(elements.elements.iter().rev().copied());
                }
                PatternKind::Struct { fields, .. } => {
                    work.extend(fields.iter().rev().map(|field| field.pattern));
                }
                PatternKind::Wildcard | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
            }
        }
        bindings
    }

    /// Borrows the type-syntax row addressed by `id`; see the [storage checks](Self#panics).
    pub fn type_ref(&self, id: TypeRefId) -> &TypeData {
        assert_eq!(id.arena(), self.arena, "foreign HIR type reference");
        let (owner, node) = &self.types[id.index()];
        assert_eq!(id.owner(), *owner, "foreign HIR body node");
        node
    }
}
