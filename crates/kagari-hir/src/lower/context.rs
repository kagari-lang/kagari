use kagari_common::Span;
use kagari_syntax::ast::AstNode;
use kagari_syntax::kind::SyntaxKind;

use crate::hir::{
    BinaryOp, BlockData, BlockId, ExprData, ExprId, ExprKind, LocalId, Module, PatternData,
    PatternId, PatternKind, PlaceData, PlaceId, PlaceKind, StmtData, StmtId, TypeData, TypeKind,
    TypeRefId,
};
use crate::source_map::SourceMap;

pub(crate) struct Lowerer {
    pub(crate) source_map: SourceMap,
    pub(crate) module: Module,
}

impl Lowerer {
    pub(crate) fn new() -> Self {
        Self {
            source_map: SourceMap::default(),
            module: Module::default(),
        }
    }

    pub(crate) fn finish(self) -> (Module, SourceMap) {
        (self.module, self.source_map)
    }

    pub(crate) fn alloc_block(&mut self, span: Span, block: BlockData) -> BlockId {
        let id = self.source_map.push_block(span);
        self.module.body.blocks.push(block);
        id
    }

    pub(crate) fn alloc_stmt(&mut self, span: Span, stmt: StmtData) -> StmtId {
        let id = self.source_map.push_stmt(span);
        self.module.body.stmts.push(stmt);
        id
    }

    pub(crate) fn alloc_expr(&mut self, span: Span, expr: ExprData) -> ExprId {
        let id = self.source_map.push_expr(span);
        self.module.body.exprs.push(expr);
        id
    }

    pub(crate) fn alloc_pattern(&mut self, span: Span, pattern: PatternData) -> PatternId {
        let id = self.source_map.push_pattern(span);
        self.module.body.patterns.push(pattern);
        id
    }

    pub(crate) fn alloc_local_id(&mut self, span: Span) -> LocalId {
        self.source_map.push_local(span)
    }

    pub(crate) fn alloc_place(&mut self, span: Span, place: PlaceData) -> PlaceId {
        let id = self.source_map.push_place(span);
        self.module.body.places.push(place);
        id
    }

    pub(crate) fn alloc_type(&mut self, span: Span, ty: TypeData) -> TypeRefId {
        let id = self.source_map.push_type(span);
        self.module.body.types.push(ty);
        id
    }

    pub(crate) fn synthetic_name_expr(&mut self, name: &str) -> ExprId {
        self.alloc_expr(
            Span::default(),
            ExprData {
                kind: ExprKind::Name(name.to_string()),
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

pub(crate) fn syntax_span(node: &impl AstNode) -> Span {
    let range = node.syntax().text_range();
    Span::new(range.start().into(), range.end().into())
}

pub(crate) fn lower_binary_op(kind: Option<SyntaxKind>) -> BinaryOp {
    match kind {
        Some(SyntaxKind::Minus) => BinaryOp::Sub,
        Some(SyntaxKind::Star) => BinaryOp::Mul,
        Some(SyntaxKind::Slash) => BinaryOp::Div,
        Some(SyntaxKind::EqEq) => BinaryOp::Eq,
        Some(SyntaxKind::NotEq) => BinaryOp::NotEq,
        Some(SyntaxKind::Lt) => BinaryOp::Lt,
        Some(SyntaxKind::Gt) => BinaryOp::Gt,
        Some(SyntaxKind::Le) => BinaryOp::Le,
        Some(SyntaxKind::Ge) => BinaryOp::Ge,
        Some(SyntaxKind::AmpAmp) => BinaryOp::AndAnd,
        Some(SyntaxKind::PipePipe) => BinaryOp::OrOr,
        _ => BinaryOp::Add,
    }
}
