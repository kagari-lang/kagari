use kagari_common::Span;

use crate::hir::{
    BlockId, ConstId, EnumId, ExprId, FunctionId, ImplId, LocalId, ModuleId, ParamId, PatternId,
    PlaceId, StmtId, StructId, TraitId, TraitMethodId, TypeRefId,
};

#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    arena: crate::hir::HirArenaId,
    owner: crate::hir::HirOwner,
    generic_param_spans: Vec<Span>,
    field_spans: std::collections::HashMap<crate::hir::FieldId, Span>,
    variant_spans: std::collections::HashMap<crate::hir::VariantId, Span>,
    function_spans: Vec<Span>,
    const_spans: Vec<Span>,
    module_spans: Vec<Span>,
    trait_spans: Vec<Span>,
    trait_method_spans: Vec<Span>,
    impl_spans: Vec<Span>,
    param_spans: Vec<Span>,
    param_owners: Vec<crate::hir::HirOwner>,
    local_spans: Vec<Span>,
    local_owners: Vec<crate::hir::HirOwner>,
    struct_spans: Vec<Span>,
    enum_spans: Vec<Span>,
    block_spans: Vec<Span>,
    block_owners: Vec<crate::hir::HirOwner>,
    expr_spans: Vec<Span>,
    expr_reference_spans: std::collections::HashMap<ExprId, Span>,
    struct_field_spans: std::collections::HashMap<ExprId, Vec<Option<Span>>>,
    expr_owners: Vec<crate::hir::HirOwner>,
    place_spans: Vec<Span>,
    place_member_spans: std::collections::HashMap<PlaceId, Span>,
    place_owners: Vec<crate::hir::HirOwner>,
    stmt_spans: Vec<Span>,
    stmt_owners: Vec<crate::hir::HirOwner>,
    pattern_spans: Vec<Span>,
    pattern_owners: Vec<crate::hir::HirOwner>,
    type_spans: Vec<Span>,
    type_owners: Vec<crate::hir::HirOwner>,
}

impl SourceMap {
    pub(crate) fn insert_variant(&mut self, id: crate::hir::VariantId, span: Span) {
        self.variant_spans.insert(id, span);
    }
    pub fn variant_span(&self, id: crate::hir::VariantId) -> Span {
        self.variant_spans[&id]
    }
    pub(crate) fn set_owner(&mut self, owner: crate::hir::HirOwner) -> crate::hir::HirOwner {
        std::mem::replace(&mut self.owner, owner)
    }

    pub(crate) fn type_id(&self, index: usize) -> TypeRefId {
        assert!(
            index < self.type_spans.len(),
            "HIR source slot out of bounds"
        );
        TypeRefId::new(self.arena, self.type_owners[index], index)
    }

    pub(crate) fn pattern_id(&self, index: usize) -> PatternId {
        assert!(
            index < self.pattern_spans.len(),
            "HIR source slot out of bounds"
        );
        PatternId::new(self.arena, self.pattern_owners[index], index)
    }

    pub(crate) fn place_id(&self, index: usize) -> PlaceId {
        assert!(
            index < self.place_spans.len(),
            "HIR source slot out of bounds"
        );
        PlaceId::new(self.arena, self.place_owners[index], index)
    }

    pub(crate) fn expr_id(&self, index: usize) -> ExprId {
        assert!(
            index < self.expr_spans.len(),
            "HIR source slot out of bounds"
        );
        ExprId::new(self.arena, self.expr_owners[index], index)
    }

    pub(crate) fn local_id(&self, index: usize) -> LocalId {
        assert!(
            index < self.local_spans.len(),
            "HIR source slot out of bounds"
        );
        LocalId::new(self.arena, self.local_owners[index], index)
    }

    pub fn arena(&self) -> crate::hir::HirArenaId {
        self.arena
    }

    pub(crate) fn push_generic_param(&mut self, span: Span) -> crate::hir::GenericParamId {
        let id = crate::hir::GenericParamId::new(self.generic_param_spans.len());
        self.generic_param_spans.push(span);
        id
    }
    pub fn generic_param_span(&self, id: crate::hir::GenericParamId) -> Span {
        self.generic_param_spans[id.index()]
    }
    pub(crate) fn type_spans(&self) -> &[Span] {
        &self.type_spans
    }
    pub(crate) fn insert_field(&mut self, id: crate::hir::FieldId, span: Span) {
        self.field_spans.insert(id, span);
    }

    pub fn field_span(&self, id: crate::hir::FieldId) -> Span {
        self.field_spans[&id]
    }

    pub fn item_span(&self, item: crate::hir::Item) -> Span {
        use crate::hir::Item;
        match item {
            Item::Function(id) => self.function_span(id),
            Item::Const(id) => self.const_span(id),
            Item::Module(id) => self.module_span(id),
            Item::Struct(id) => self.struct_span(id),
            Item::Enum(id) => self.enum_span(id),
            Item::Trait(id) => self.trait_span(id),
            Item::Impl(id) => self.impl_span(id),
        }
    }

    pub(crate) fn pattern_spans(&self) -> &[Span] {
        &self.pattern_spans
    }
    pub(crate) fn expr_spans(&self) -> &[Span] {
        &self.expr_spans
    }
    pub(crate) fn local_spans(&self) -> &[Span] {
        &self.local_spans
    }
    pub(crate) fn place_spans(&self) -> &[Span] {
        &self.place_spans
    }
    pub(crate) fn push_function(&mut self, span: Span) -> FunctionId {
        let id = FunctionId::new(self.function_spans.len());
        self.function_spans.push(span);
        id
    }

    pub(crate) fn push_const(&mut self, span: Span) -> ConstId {
        let id = ConstId::new(self.const_spans.len());
        self.const_spans.push(span);
        id
    }

    pub(crate) fn push_module(&mut self, span: Span) -> ModuleId {
        let id = ModuleId::new(self.module_spans.len());
        self.module_spans.push(span);
        id
    }

    pub(crate) fn push_trait(&mut self, span: Span) -> TraitId {
        let id = TraitId::new(self.trait_spans.len());
        self.trait_spans.push(span);
        id
    }

    pub(crate) fn push_trait_method(&mut self, span: Span) -> TraitMethodId {
        let id = TraitMethodId::new(self.trait_method_spans.len());
        self.trait_method_spans.push(span);
        id
    }

    pub(crate) fn push_impl(&mut self, span: Span) -> ImplId {
        let id = ImplId::new(self.impl_spans.len());
        self.impl_spans.push(span);
        id
    }

    pub(crate) fn push_param(&mut self, span: Span) -> ParamId {
        let id = ParamId::new(self.arena, self.owner, self.param_spans.len());
        self.param_spans.push(span);
        self.param_owners.push(self.owner);
        id
    }

    pub(crate) fn push_local(&mut self, span: Span) -> LocalId {
        let id = LocalId::new(self.arena, self.owner, self.local_spans.len());
        self.local_spans.push(span);
        self.local_owners.push(self.owner);
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
        let id = BlockId::new(self.arena, self.owner, self.block_spans.len());
        self.block_spans.push(span);
        self.block_owners.push(self.owner);
        id
    }

    pub(crate) fn push_expr(&mut self, span: Span) -> ExprId {
        let id = ExprId::new(self.arena, self.owner, self.expr_spans.len());
        self.expr_spans.push(span);
        self.expr_owners.push(self.owner);
        id
    }

    pub(crate) fn insert_expr_reference(&mut self, id: ExprId, span: Span) {
        self.expr_reference_spans.insert(id, span);
    }

    pub fn expr_reference_span(&self, id: ExprId) -> Option<Span> {
        self.expr_reference_spans.get(&id).copied()
    }

    pub(crate) fn insert_struct_fields(&mut self, id: ExprId, spans: Vec<Option<Span>>) {
        self.struct_field_spans.insert(id, spans);
    }

    pub fn struct_field_spans(&self, id: ExprId) -> Option<&[Option<Span>]> {
        self.struct_field_spans.get(&id).map(Vec::as_slice)
    }

    pub(crate) fn push_place(&mut self, span: Span) -> PlaceId {
        let id = PlaceId::new(self.arena, self.owner, self.place_spans.len());
        self.place_spans.push(span);
        self.place_owners.push(self.owner);
        id
    }

    pub(crate) fn insert_place_member(&mut self, id: PlaceId, span: Span) {
        self.place_member_spans.insert(id, span);
    }

    pub fn place_member_span(&self, id: PlaceId) -> Option<Span> {
        self.place_member_spans.get(&id).copied()
    }

    pub(crate) fn push_stmt(&mut self, span: Span) -> StmtId {
        let id = StmtId::new(self.arena, self.owner, self.stmt_spans.len());
        self.stmt_spans.push(span);
        self.stmt_owners.push(self.owner);
        id
    }

    pub(crate) fn push_pattern(&mut self, span: Span) -> PatternId {
        let id = PatternId::new(self.arena, self.owner, self.pattern_spans.len());
        self.pattern_spans.push(span);
        self.pattern_owners.push(self.owner);
        id
    }

    pub(crate) fn push_type(&mut self, span: Span) -> TypeRefId {
        let id = TypeRefId::new(self.arena, self.owner, self.type_spans.len());
        self.type_spans.push(span);
        self.type_owners.push(self.owner);
        id
    }

    pub fn function_span(&self, id: FunctionId) -> Span {
        self.function_spans[id.index()]
    }

    pub fn module_span(&self, id: ModuleId) -> Span {
        self.module_spans[id.index()]
    }

    pub fn trait_span(&self, id: TraitId) -> Span {
        self.trait_spans[id.index()]
    }

    pub fn trait_method_span(&self, id: TraitMethodId) -> Span {
        self.trait_method_spans[id.index()]
    }

    pub fn impl_span(&self, id: ImplId) -> Span {
        self.impl_spans[id.index()]
    }

    pub fn const_span(&self, id: ConstId) -> Span {
        self.const_spans[id.index()]
    }

    pub fn param_span(&self, id: ParamId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.param_owners[id.index()],
            "foreign HIR body source range"
        );
        self.param_spans[id.index()]
    }

    pub fn local_span(&self, id: LocalId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.local_owners[id.index()],
            "foreign HIR body source range"
        );
        self.local_spans[id.index()]
    }

    pub fn struct_span(&self, id: StructId) -> Span {
        self.struct_spans[id.index()]
    }

    pub fn enum_span(&self, id: EnumId) -> Span {
        self.enum_spans[id.index()]
    }

    pub fn block_span(&self, id: BlockId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.block_owners[id.index()],
            "foreign HIR body source range"
        );
        self.block_spans[id.index()]
    }

    pub fn expr_span(&self, id: ExprId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.expr_owners[id.index()],
            "foreign HIR body source range"
        );
        self.expr_spans[id.index()]
    }

    pub fn place_span(&self, id: PlaceId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.place_owners[id.index()],
            "foreign HIR body source range"
        );
        self.place_spans[id.index()]
    }

    pub fn stmt_span(&self, id: StmtId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.stmt_owners[id.index()],
            "foreign HIR body source range"
        );
        self.stmt_spans[id.index()]
    }

    pub fn pattern_span(&self, id: PatternId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.pattern_owners[id.index()],
            "foreign HIR body source range"
        );
        self.pattern_spans[id.index()]
    }

    pub fn type_span(&self, id: TypeRefId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.type_owners[id.index()],
            "foreign HIR body source range"
        );
        self.type_spans[id.index()]
    }
}
