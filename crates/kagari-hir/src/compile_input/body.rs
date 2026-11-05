//! Owner-checked structural body access with matching semantic and source facts.

use crate::{
    CheckedAnalysis,
    compile_input::{environment::CheckedEnvironment, program::AdmissionError},
    hir::{
        body::Body,
        expr::{ClosureParam, ExprData, ExprKind},
        ids::{BlockId, BodyOwner, ExprId, HirArenaId, HirOwner, PatternId, PlaceId, StmtId},
        pattern::PatternData,
        place::PlaceData,
        stmt::{BlockData, StmtData},
    },
    resolver::resolved::ResolvedNames,
    semantic::signatures::TypedFunction,
    source_map::SourceMap,
    typeck::table::TypeTable,
};
pub mod bindings;
pub mod facts;
pub mod flow;

use kagari_common::{
    identity::table::{DefinitionTable, module::ModuleHandle},
    span::Span,
};

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
    /// Nested closure/async expression within that owner, or `None` for its root.
    pub closure: Option<ExprId>,
}

/// Structural entry of a function or initializer; no execution has occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyRoot {
    /// A declared function's structural block.
    Block(BlockId),
    /// A constant/runtime static initializer or nested callable expression root.
    Expression(ExprId),
}

/// Immutable source nodes, selected facts and origins for one admitted body.
///
/// For `fn add(x: i32) -> i32 { x + 1 }`, the root is its block, whose tail is
/// the addition expression. `expression` borrows that node; `expression_facts`
/// projects its semantic type and any selected operator contract.
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
    signature: Option<&'a TypedFunction>,
    definitions: &'a DefinitionTable,
    environment: CheckedEnvironment<'a>,
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
                closure: None,
            },
            root,
            nodes: &module.lowered.module.body,
            types: module.typed.type_table(),
            names: &module.names,
            origins: &module.lowered.source_map,
            definitions: module.definitions(),
            environment: CheckedEnvironment::new(module),
            signature: match owner {
                BodyOwner::Function(id) => module.typed.signatures().function(id),
                _ => None,
            },
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

    /// Returns the function block, initializer or nested callable root to elaborate.
    pub fn root(self) -> BodyRoot {
        self.root
    }

    /// Borrows the exact declaration snapshot used to check this body's facts.
    ///
    /// Elaboration obtains its environment here, rather than accepting a separate
    /// environment that could belong to an older revision of the same module.
    pub fn environment(self) -> CheckedEnvironment<'a> {
        self.environment
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

    /// Returns the original source extent of an expression in this body.
    pub fn expression_span(self, id: ExprId) -> Option<Span> {
        self.expression(id)?;
        Some(self.origins.expr_span(id))
    }

    /// Selects a nested callable root without allocating another source arena.
    ///
    /// Closure captures can refer to enclosing bindings with the same allocation
    /// owner. The nested key identifies the callable, while traversal starts at
    /// its own expression root and never enters another named function's arena.
    pub fn closure(self, id: ExprId) -> Option<Self> {
        let body = match &self.expression(id)?.kind {
            ExprKind::Closure { body, .. } | ExprKind::AsyncBlock { body } => *body,
            _ => return None,
        };
        self.expression(body)?;
        Some(Self {
            key: BodyKey {
                closure: Some(id),
                ..self.key
            },
            root: BodyRoot::Expression(body),
            ..self
        })
    }

    /// Borrows explicitly bound closure parameters; async blocks have no parameters.
    pub fn closure_parameters(self) -> Option<&'a [ClosureParam]> {
        match &self.expression(self.key.closure?)?.kind {
            ExprKind::Closure { params, .. } => Some(params),
            ExprKind::AsyncBlock { .. } => Some(&[]),
            _ => None,
        }
    }

    /// Borrows the enclosing declaration signature, including parameters captured by closures.
    pub fn enclosing_signature(self) -> Option<&'a TypedFunction> {
        self.signature
    }

    /// Returns the source extent of a block belonging to this input.
    pub fn block_span(self, id: BlockId) -> Option<Span> {
        self.block(id)?;
        Some(self.origins.block_span(id))
    }

    /// Returns the source extent of a statement belonging to this input.
    pub fn statement_span(self, id: StmtId) -> Option<Span> {
        self.statement(id)?;
        Some(self.origins.stmt_span(id))
    }

    /// Returns the source extent of a mutation target belonging to this input.
    pub fn place_span(self, id: PlaceId) -> Option<Span> {
        self.place(id)?;
        Some(self.origins.place_span(id))
    }

    /// Returns the source extent of a pattern belonging to this input.
    pub fn pattern_span(self, id: PatternId) -> Option<Span> {
        self.pattern(id)?;
        Some(self.origins.pattern_span(id))
    }
}
