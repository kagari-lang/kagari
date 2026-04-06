use kagari_common::Span;

use crate::hir::{
    BlockId, EnumId, ExprId, FunctionId, LocalId, ParamId, PatternId, PlaceId, StmtId, StructId,
    TypeRefId,
};

#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    function_spans: Vec<Span>,
    param_spans: Vec<Span>,
    local_spans: Vec<Span>,
    struct_spans: Vec<Span>,
    enum_spans: Vec<Span>,
    block_spans: Vec<Span>,
    expr_spans: Vec<Span>,
    place_spans: Vec<Span>,
    stmt_spans: Vec<Span>,
    pattern_spans: Vec<Span>,
    type_spans: Vec<Span>,
}

impl SourceMap {
    pub(crate) fn push_function(&mut self, span: Span) -> FunctionId {
        let id = FunctionId::new(self.function_spans.len());
        self.function_spans.push(span);
        id
    }

    pub(crate) fn push_param(&mut self, span: Span) -> ParamId {
        let id = ParamId::new(self.param_spans.len());
        self.param_spans.push(span);
        id
    }

    pub(crate) fn push_local(&mut self, span: Span) -> LocalId {
        let id = LocalId::new(self.local_spans.len());
        self.local_spans.push(span);
        id
    }

    pub(crate) fn push_struct(&mut self, span: Span) -> StructId {
        let id = StructId::new(self.struct_spans.len());
        self.struct_spans.push(span);
        id
    }

    pub(crate) fn push_enum(&mut self, span: Span) -> EnumId {
        let id = EnumId::new(self.enum_spans.len());
        self.enum_spans.push(span);
        id
    }

    pub(crate) fn push_block(&mut self, span: Span) -> BlockId {
        let id = BlockId::new(self.block_spans.len());
        self.block_spans.push(span);
        id
    }

    pub(crate) fn push_expr(&mut self, span: Span) -> ExprId {
        let id = ExprId::new(self.expr_spans.len());
        self.expr_spans.push(span);
        id
    }

    pub(crate) fn push_place(&mut self, span: Span) -> PlaceId {
        let id = PlaceId::new(self.place_spans.len());
        self.place_spans.push(span);
        id
    }

    pub(crate) fn push_stmt(&mut self, span: Span) -> StmtId {
        let id = StmtId::new(self.stmt_spans.len());
        self.stmt_spans.push(span);
        id
    }

    pub(crate) fn push_pattern(&mut self, span: Span) -> PatternId {
        let id = PatternId::new(self.pattern_spans.len());
        self.pattern_spans.push(span);
        id
    }

    pub(crate) fn push_type(&mut self, span: Span) -> TypeRefId {
        let id = TypeRefId::new(self.type_spans.len());
        self.type_spans.push(span);
        id
    }

    pub fn function_span(&self, id: FunctionId) -> Span {
        self.function_spans[id.index()]
    }

    pub fn param_span(&self, id: ParamId) -> Span {
        self.param_spans[id.index()]
    }

    pub fn local_span(&self, id: LocalId) -> Span {
        self.local_spans[id.index()]
    }

    pub fn struct_span(&self, id: StructId) -> Span {
        self.struct_spans[id.index()]
    }

    pub fn enum_span(&self, id: EnumId) -> Span {
        self.enum_spans[id.index()]
    }

    pub fn block_span(&self, id: BlockId) -> Span {
        self.block_spans[id.index()]
    }

    pub fn expr_span(&self, id: ExprId) -> Span {
        self.expr_spans[id.index()]
    }

    pub fn place_span(&self, id: PlaceId) -> Span {
        self.place_spans[id.index()]
    }

    pub fn stmt_span(&self, id: StmtId) -> Span {
        self.stmt_spans[id.index()]
    }

    pub fn pattern_span(&self, id: PatternId) -> Span {
        self.pattern_spans[id.index()]
    }

    pub fn type_span(&self, id: TypeRefId) -> Span {
        self.type_spans[id.index()]
    }
}
