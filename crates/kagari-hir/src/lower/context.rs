//! Shared HIR construction state, synchronized allocation and source-range helpers.

use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId, span::Span};
use kagari_syntax::ast::{borrow::AstNode, expr::BinaryOp as AstBinaryOp};
use std::mem;

use crate::hir::{
    expr::{ExprData, ExprKind, ops::BinaryOp},
    ids::{
        BlockId, ExprId, FunctionId, GenericParamId, HirOwner, LocalId, PatternId, PlaceId, StmtId,
        TypeRefId,
    },
    item::{Module, behavior::GenericParam, function::Function},
    nodes::Local,
    origin::NodeOrigin,
    pattern::{PatternData, PatternKind},
    place::{PlaceData, PlaceKind},
    stmt::{BlockData, StmtData, StmtKind},
    ty::{TypeData, TypeKind},
};

/// Construction state for the active HIR owner and structured declaration.
///
/// Allocated records belong to the matching module and carry their own origins.
pub(crate) struct Lowerer {
    /// Reserved declaration slots, finalized only when all nested lowering completes.
    pub(crate) functions: Vec<Option<Function>>,
    /// Block currently receiving statements; saved/restored across nested blocks.
    pub(crate) current_block: Option<BlockId>,
    /// Cancellation observed by traversal loops; publication callers reject cancelled work.
    pub(crate) cancel: CancellationToken,
    owner: HirOwner,
    structured_owner: Option<DefinitionId>,
    /// Declarations and node rows being built; unresolved until later analysis.
    pub(crate) module: Module,
}

impl Lowerer {
    /// Creates module storage with a fresh lowering identity.
    pub(crate) fn new(cancel: CancellationToken) -> Self {
        Self {
            functions: Vec::new(),
            current_block: None,
            cancel,
            owner: HirOwner::Declaration,
            structured_owner: None,
            module: Module::default(),
        }
    }

    /// Publishes the complete module after traversal.
    pub(crate) fn finish(mut self) -> Module {
        self.module.functions = self
            .functions
            .into_iter()
            .map(|slot| slot.expect("completed function lowering"))
            .collect();
        // Trait/impl lowering visits member categories rather than authored order.
        self.module
            .attributes
            .sort_by_key(|attribute| attribute.span.start);
        self.module
    }

    pub(crate) fn set_owner(&mut self, owner: HirOwner) -> HirOwner {
        mem::replace(&mut self.owner, owner)
    }

    pub(crate) fn set_structured_origin(
        &mut self,
        declaration: DefinitionId,
    ) -> Option<DefinitionId> {
        self.structured_owner.replace(declaration)
    }

    pub(crate) fn restore_structured_origin(&mut self, owner: Option<DefinitionId>) {
        self.structured_owner = owner;
    }

    pub(crate) fn origin(&self, span: Span) -> NodeOrigin {
        match self.structured_owner {
            Some(declaration) => NodeOrigin::Structured { declaration },
            None => NodeOrigin::Source(span),
        }
    }

    /// Reserves a function slot before lowering its children.
    pub(crate) fn reserve_function(&mut self) -> FunctionId {
        let id = FunctionId::new(self.functions.len());
        self.functions.push(None);
        id
    }

    /// Fills a reserved function slot exactly once.
    pub(crate) fn finish_function(&mut self, function: Function) {
        let slot = &mut self.functions[function.id.index()];
        assert!(slot.is_none());
        *slot = Some(function);
    }

    pub(crate) fn alloc_block(&mut self, span: Span, tail_expr: Option<ExprId>) -> BlockId {
        let origin = self.origin(span);
        let owner = self.owner;
        let nodes = &mut self.module.nodes;
        let id = BlockId::new(nodes.arena(), owner, nodes.blocks.len());
        nodes.blocks.push((
            owner,
            BlockData {
                origin,
                statements: Default::default(),
                tail_expr,
            },
        ));
        id
    }

    pub(crate) fn alloc_stmt(&mut self, span: Span, kind: StmtKind) -> StmtId {
        let origin = self.origin(span);
        let owner = self.owner;
        let nodes = &mut self.module.nodes;
        let id = StmtId::new(nodes.arena(), owner, nodes.stmts.len());
        nodes.stmts.push((owner, StmtData { origin, kind }));
        id
    }

    pub(crate) fn alloc_expr(&mut self, span: Span, kind: ExprKind) -> ExprId {
        let origin = self.origin(span);
        let owner = self.owner;
        let nodes = &mut self.module.nodes;
        let id = ExprId::new(nodes.arena(), owner, nodes.exprs.len());
        nodes.exprs.push((owner, ExprData { origin, kind }));
        id
    }

    pub(crate) fn alloc_pattern(&mut self, span: Span, kind: PatternKind) -> PatternId {
        let origin = self.origin(span);
        let owner = self.owner;
        let nodes = &mut self.module.nodes;
        let id = PatternId::new(nodes.arena(), owner, nodes.patterns.len());
        nodes.patterns.push((owner, PatternData { origin, kind }));
        id
    }

    pub(crate) fn alloc_generic_param(&mut self, record: GenericParam) -> GenericParamId {
        let id = GenericParamId::new(self.module.generic_params.len());
        self.module.generic_params.push(record);
        id
    }

    /// Allocates an authoritative name record for a provisional local.
    pub(crate) fn alloc_local(&mut self, span: Span, name: String) -> LocalId {
        let origin = self.origin(span);
        let nodes = &mut self.module.nodes;
        let owner = self.owner;
        let id = LocalId::new(nodes.arena(), owner, nodes.locals.len());
        nodes.locals.push((owner, Local { name, origin }));
        id
    }

    pub(crate) fn alloc_place(&mut self, span: Span, kind: PlaceKind) -> PlaceId {
        let origin = self.origin(span);
        let owner = self.owner;
        let nodes = &mut self.module.nodes;
        let id = PlaceId::new(nodes.arena(), owner, nodes.places.len());
        nodes.places.push((owner, PlaceData { origin, kind }));
        id
    }

    pub(crate) fn alloc_type(&mut self, span: Span, kind: TypeKind) -> TypeRefId {
        let origin = self.origin(span);
        let owner = self.owner;
        let nodes = &mut self.module.nodes;
        let id = TypeRefId::new(nodes.arena(), owner, nodes.types.len());
        nodes.types.push((owner, TypeData { origin, kind }));
        id
    }

    /// Allocates a recovery expression with an empty synthetic source range.
    pub(crate) fn missing_expr(&mut self) -> ExprId {
        self.alloc_expr(Span::default(), ExprKind::Missing)
    }

    pub(crate) fn synthetic_name_pattern(&mut self, name: &str) -> PatternId {
        let local = self.alloc_local(Span::default(), name.to_string());
        self.alloc_pattern(Span::default(), PatternKind::Name { local })
    }

    pub(crate) fn synthetic_name_place(&mut self, name: &str) -> PlaceId {
        self.alloc_place(Span::default(), PlaceKind::Name(name.to_string()))
    }

    pub(crate) fn synthetic_named_type(&mut self, name: &str) -> TypeRefId {
        self.alloc_type(Span::default(), TypeKind::Named(name.to_string()))
    }
}

/// Returns the authored node range, excluding outer trivia owned by syntax layout.
/// Semantic locations and replacement extents must stay within written tokens.
pub(crate) fn syntax_span(node: &impl AstNode) -> Span {
    node.body_span()
}

/// The body's constant-time first/last lexical boundaries, including internal recovery.
pub(crate) fn token_span(node: &impl AstNode) -> Span {
    node.body_span()
}

pub(crate) fn lower_binary_op(kind: AstBinaryOp) -> BinaryOp {
    match kind {
        AstBinaryOp::Add => BinaryOp::Add,
        AstBinaryOp::Subtract => BinaryOp::Sub,
        AstBinaryOp::Multiply => BinaryOp::Mul,
        AstBinaryOp::Divide => BinaryOp::Div,
        AstBinaryOp::Remainder => BinaryOp::Rem,
        AstBinaryOp::BitAnd => BinaryOp::BitAnd,
        AstBinaryOp::BitOr => BinaryOp::BitOr,
        AstBinaryOp::BitXor => BinaryOp::BitXor,
        AstBinaryOp::ShiftLeft => BinaryOp::Shl,
        AstBinaryOp::ShiftRight => BinaryOp::Shr,
        AstBinaryOp::And => BinaryOp::AndAnd,
        AstBinaryOp::Or => BinaryOp::OrOr,
        AstBinaryOp::Equal => BinaryOp::Eq,
        AstBinaryOp::NotEqual => BinaryOp::NotEq,
        AstBinaryOp::IdentityEqual => BinaryOp::IdentityEq,
        AstBinaryOp::IdentityNotEqual => BinaryOp::IdentityNotEq,
        AstBinaryOp::Less => BinaryOp::Lt,
        AstBinaryOp::LessEqual => BinaryOp::Le,
        AstBinaryOp::Greater => BinaryOp::Gt,
        AstBinaryOp::GreaterEqual => BinaryOp::Ge,
    }
}
