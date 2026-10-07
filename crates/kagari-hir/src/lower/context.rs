//! Shared HIR construction state, synchronized allocation and source-range helpers.

use kagari_common::{cancellation::CancellationToken, span::Span};
use kagari_syntax::{ast::traits::AstNode, kind::SyntaxKind};

use crate::{
    hir::{
        expr::{ExprData, ExprKind, ops::BinaryOp},
        ids::{BlockId, ExprId, LocalId, PatternId, PlaceId, StmtId, TypeRefId},
        item::Module,
        pattern::{PatternData, PatternKind},
        place::{PlaceData, PlaceKind},
        stmt::{BlockData, StmtData},
        ty::{TypeData, TypeKind},
    },
    source_map::SourceMap,
};

/// Mutable construction state whose source-map slots and body rows advance together.
///
/// Allocation obtains an ID from `SourceMap` first, then appends the corresponding
/// payload to `Module.body`. Keep those vectors aligned. Owner changes are made on
/// the source map by item lowering and restored after the nested construction.
pub(crate) struct Lowerer {
    /// Cancellation observed by traversal loops; publication callers reject cancelled work.
    pub(crate) cancel: CancellationToken,
    /// Arena identity, active owner and allocation-aligned byte ranges.
    pub(crate) source_map: SourceMap,
    /// Declarations and node rows being built; unresolved until later analysis.
    pub(crate) module: Module,
}

impl Lowerer {
    /// Creates fresh storage and gives the body the source map's arena identity.
    pub(crate) fn new(cancel: CancellationToken) -> Self {
        let source_map = SourceMap::default();
        let mut module = Module::default();
        module.body.arena = source_map.arena();
        Self {
            cancel,
            source_map,
            module,
        }
    }

    /// Moves out the matching module/map pair after traversal.
    pub(crate) fn finish(self) -> (Module, SourceMap) {
        (self.module, self.source_map)
    }

    /// Allocates a source-map ID and appends its owner-tagged block payload at the matching index.
    pub(crate) fn alloc_block(&mut self, span: Span, block: BlockData) -> BlockId {
        let id = self.source_map.push_block(span);
        self.module.body.blocks.push((id.owner(), block));
        id
    }

    /// Allocates a source-map ID and appends its owner-tagged stmt payload at the matching index.
    pub(crate) fn alloc_stmt(&mut self, span: Span, stmt: StmtData) -> StmtId {
        let id = self.source_map.push_stmt(span);
        self.module.body.stmts.push((id.owner(), stmt));
        id
    }

    /// Allocates a source-map ID and appends its owner-tagged expr payload at the matching index.
    pub(crate) fn alloc_expr(&mut self, span: Span, expr: ExprData) -> ExprId {
        let id = self.source_map.push_expr(span);
        self.module.body.exprs.push((id.owner(), expr));
        id
    }

    /// Allocates a source-map ID and appends its owner-tagged pattern payload at the matching index.
    pub(crate) fn alloc_pattern(&mut self, span: Span, pattern: PatternData) -> PatternId {
        let id = self.source_map.push_pattern(span);
        self.module.body.patterns.push((id.owner(), pattern));
        id
    }

    /// Allocates a local binding identity and span; the binding payload lives at its syntax owner.
    pub(crate) fn alloc_local_id(&mut self, span: Span) -> LocalId {
        self.source_map.push_local(span)
    }

    /// Allocates a source-map ID and appends its owner-tagged place payload at the matching index.
    pub(crate) fn alloc_place(&mut self, span: Span, place: PlaceData) -> PlaceId {
        let id = self.source_map.push_place(span);
        self.module.body.places.push((id.owner(), place));
        id
    }

    /// Allocates a source-map ID and appends its owner-tagged type payload at the matching index.
    pub(crate) fn alloc_type(&mut self, span: Span, ty: TypeData) -> TypeRefId {
        let id = self.source_map.push_type(span);
        self.module.body.types.push((id.owner(), ty));
        id
    }

    /// Allocates a recovery expression with an empty synthetic source range.
    pub(crate) fn missing_expr(&mut self) -> ExprId {
        self.alloc_expr(
            Span::default(),
            ExprData {
                kind: ExprKind::Missing,
            },
        )
    }

    pub(crate) fn synthetic_name_pattern(&mut self, name: &str) -> PatternId {
        let local = self.alloc_local_id(Span::default());
        self.alloc_pattern(
            Span::default(),
            PatternData {
                kind: PatternKind::Name {
                    name: name.to_string(),
                    local,
                },
            },
        )
    }

    pub(crate) fn synthetic_name_place(&mut self, name: &str) -> PlaceId {
        self.alloc_place(
            Span::default(),
            PlaceData {
                kind: PlaceKind::Name(name.to_string()),
            },
        )
    }

    pub(crate) fn synthetic_named_type(&mut self, name: &str) -> TypeRefId {
        self.alloc_type(
            Span::default(),
            TypeData {
                kind: TypeKind::Named(name.to_string()),
            },
        )
    }
}

/// Returns the full AST node's half-open byte range, including retained trivia.
pub(crate) fn syntax_span(node: &impl AstNode) -> Span {
    let range = node.syntax().text_range();
    Span::new(range.start().into(), range.end().into())
}

/// Returns first-to-last nontrivia token bytes, falling back to the node range when empty.
pub(crate) fn token_span(node: &impl AstNode) -> Span {
    let mut tokens = node
        .syntax()
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !token.kind().is_trivia());
    let Some(first) = tokens.next() else {
        return syntax_span(node);
    };
    let last = tokens.last().unwrap_or_else(|| first.clone());
    Span::new(
        first.text_range().start().into(),
        last.text_range().end().into(),
    )
}

pub(crate) fn lower_binary_op(kind: Option<SyntaxKind>) -> BinaryOp {
    match kind {
        Some(SyntaxKind::Minus) => BinaryOp::Sub,
        Some(SyntaxKind::Star) => BinaryOp::Mul,
        Some(SyntaxKind::Slash) => BinaryOp::Div,
        Some(SyntaxKind::Percent) => BinaryOp::Rem,
        Some(SyntaxKind::Amp) => BinaryOp::BitAnd,
        Some(SyntaxKind::Pipe) => BinaryOp::BitOr,
        Some(SyntaxKind::Caret) => BinaryOp::BitXor,
        Some(SyntaxKind::Shl) => BinaryOp::Shl,
        Some(SyntaxKind::Shr) => BinaryOp::Shr,

        Some(SyntaxKind::EqEq) => BinaryOp::Eq,
        Some(SyntaxKind::NotEq) => BinaryOp::NotEq,
        Some(SyntaxKind::IdentityEq) => BinaryOp::IdentityEq,
        Some(SyntaxKind::IdentityNotEq) => BinaryOp::IdentityNotEq,
        Some(SyntaxKind::Lt) => BinaryOp::Lt,
        Some(SyntaxKind::Gt) => BinaryOp::Gt,
        Some(SyntaxKind::Le) => BinaryOp::Le,
        Some(SyntaxKind::Ge) => BinaryOp::Ge,
        Some(SyntaxKind::AmpAmp) => BinaryOp::AndAnd,
        Some(SyntaxKind::PipePipe) => BinaryOp::OrOr,
        _ => BinaryOp::Add,
    }
}
