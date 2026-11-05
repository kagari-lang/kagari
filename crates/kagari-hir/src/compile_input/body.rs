//! Owner-checked structural body access with matching semantic and source facts.

use crate::{
    CheckedAnalysis,
    compile_input::program::AdmissionError,
    hir::{
        body::Body,
        expr::ExprData,
        ids::{BlockId, BodyOwner, ExprId, HirArenaId, HirOwner, PatternId, PlaceId, StmtId},
        pattern::PatternData,
        place::PlaceData,
        stmt::{BlockData, StmtData},
    },
    resolver::resolved::{ResolvedName, ResolvedNames},
    source_map::SourceMap,
    typeck::table::{ResolvedCall, TypeTable},
    types::TypeId,
};
use kagari_common::{identity::table::module::ModuleHandle, span::Span};

/// Identity of a source body in one admitted module and lowering.
///
/// The fields form a lookup key, not compilation authority. Fresh source lowering
/// changes the arena even if the declaration keeps the same function slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BodyKey {
    /// Checked semantic module identity, independent of source path spelling.
    pub module: ModuleHandle,
    /// Lowering allocation that owns every node used by this body.
    pub arena: HirArenaId,
    /// Function or initializer declaration within that lowering.
    pub owner: BodyOwner,
}

/// Structural entry of a function or initializer; no execution has occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyRoot {
    /// A declared function's structural block.
    Block(BlockId),
    /// A constant or runtime static initializer.
    Expression(ExprId),
}

/// Immutable source nodes, selected facts and origins for one admitted body.
///
/// For `fn add(x: i32) -> i32 { x + 1 }`, the root is its block, whose tail is
/// the addition expression. `expression` borrows that node; `expression_type`
/// gives its semantic type, and `call` carries any selected operator contract.
/// A callee's member syntax need not itself be a value operation. Elaboration
/// validates the facts required by each operation, not every optional table row.
///
/// IDs from another owner or arena return `None`, even inside the same module.
/// Construction is private to closure admission; recovery results cannot create
/// this view. No method exposes the full analysis record or mutable type table.
#[derive(Clone, Copy)]
pub struct CheckedBodyInput<'a> {
    key: BodyKey,
    root: BodyRoot,
    nodes: &'a Body,
    types: &'a TypeTable,
    names: &'a ResolvedNames,
    origins: &'a SourceMap,
}

impl<'a> CheckedBodyInput<'a> {
    pub(super) fn new(
        module: &'a CheckedAnalysis,
        handle: ModuleHandle,
        owner: BodyOwner,
        root: BodyRoot,
    ) -> Result<Self, AdmissionError> {
        let body = Self {
            key: BodyKey {
                module: handle,
                arena: module.lowered.module.body.arena(),
                owner,
            },
            root,
            nodes: &module.lowered.module.body,
            types: module.typed.type_table(),
            names: &module.names,
            origins: &module.lowered.source_map,
        };
        let exists = match root {
            BodyRoot::Block(id) => body.block(id).is_some(),
            BodyRoot::Expression(id) => body.expression(id).is_some(),
        };
        if !exists {
            return Err(AdmissionError::InvalidBody {
                key: body.key,
                reason: "root does not belong to the admitted body",
            });
        }
        Ok(body)
    }

    /// Returns the source module/arena/owner key retained by this input.
    pub fn key(self) -> BodyKey {
        self.key
    }

    /// Returns the function block or initializer expression to elaborate.
    pub fn root(self) -> BodyRoot {
        self.root
    }

    fn owns(self, arena: HirArenaId, owner: HirOwner) -> bool {
        arena == self.key.arena && owner == HirOwner::Body(self.key.owner)
    }

    /// Borrows a block from this body; foreign IDs and absent slots return `None`.
    pub fn block(self, id: BlockId) -> Option<&'a BlockData> {
        let (owner, node) = self.nodes.blocks.get(id.index())?;
        (self.owns(id.arena(), id.owner()) && *owner == id.owner()).then_some(node)
    }

    /// Borrows a statement from this body, retaining its source-order child links.
    pub fn statement(self, id: StmtId) -> Option<&'a StmtData> {
        let (owner, node) = self.nodes.stmts.get(id.index())?;
        (self.owns(id.arena(), id.owner()) && *owner == id.owner()).then_some(node)
    }

    /// Borrows an expression from this body without performing semantic selection.
    pub fn expression(self, id: ExprId) -> Option<&'a ExprData> {
        let (owner, node) = self.nodes.exprs.get(id.index())?;
        (self.owns(id.arena(), id.owner()) && *owner == id.owner()).then_some(node)
    }

    /// Borrows an assignment target from this body; evaluation order remains structural.
    pub fn place(self, id: PlaceId) -> Option<&'a PlaceData> {
        let (owner, node) = self.nodes.places.get(id.index())?;
        (self.owns(id.arena(), id.owner()) && *owner == id.owner()).then_some(node)
    }

    /// Borrows a pattern from this body with its binding and alternative structure.
    pub fn pattern(self, id: PatternId) -> Option<&'a PatternData> {
        let (owner, node) = self.nodes.patterns.get(id.index())?;
        (self.owns(id.arena(), id.owner()) && *owner == id.owner()).then_some(node)
    }

    /// Returns a recorded value type, or `None` for non-value syntax/foreign IDs.
    pub fn expression_type(self, id: ExprId) -> Option<TypeId> {
        self.expression(id)?;
        self.types.expr_type(id)
    }

    /// Returns the selected call/operator contract for this expression, if applicable.
    pub fn call(self, id: ExprId) -> Option<ResolvedCall> {
        self.expression(id)?;
        self.types.call_resolution(id)
    }

    /// Returns the already resolved binding of an expression, if applicable.
    pub fn binding(self, id: ExprId) -> Option<ResolvedName> {
        self.expression(id)?;
        self.names.expr_resolution(id)
    }

    /// Returns the original source extent of an expression in this body.
    pub fn expression_span(self, id: ExprId) -> Option<Span> {
        self.expression(id)?;
        Some(self.origins.expr_span(id))
    }
}
