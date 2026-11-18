//! Allocation-aligned byte ranges and diagnostic/debug origins for lowered HIR.

use crate::collections::HashMap;
use crate::hir::{
    attribute::AttributeTarget,
    ids::{
        BlockId, BodyOwner, ConstId, EnumId, ExprId, FieldId, FunctionId, GenericParamId, HirOwner,
        ImplId, LocalId, LoweringId, ModuleId, NativeTypeId, ParamId, PatternId, PlaceId, StaticId,
        StmtId, StructId, TraitId, TraitMethodId, TypeAliasId, TypeRefId, VariantId,
    },
    item::Item,
};
pub mod origin;

use self::origin::OriginTarget;
use kagari_common::{identity::table::DefinitionId, span::Span};
use std::mem;

/// Source byte ranges and ID allocation state for one [`crate::lower::LoweredModule`].
///
/// This owns origins, not source text. Authored ranges refer to the matching
/// `LoweredModule::source_surface()`. Structured nodes have declaration-ID origins
/// and no source surface; use `origin()` / `source_span()` for attribution.
/// Legacy span accessors return empty diagnostic anchors for structured nodes,
/// which must never be interpreted as authored or implementation-source evidence.
/// The lowerer appends dense spans and payload rows together; their indices must agree.
/// Parameters instead register their function-local identities explicitly.
/// `set_owner` switches the owner attached to subsequently allocated local IDs.
///
/// ```text
/// Lowerer::alloc_expr(span, data)
/// +-- SourceMap::push_expr -> ExprId { arena, owner, index: expr_spans.len() }
/// |   +-- expr_spans[index] = span
/// |   `-- expr_owners[index] = owner
/// `-- Module.body.exprs[index] = (owner, data)
///
/// SourceMap::expr_span(id) -> arena/owner checks -> expr_spans[id.index()]
/// LoweredModule.source.text()[span.start..span.end] -> corresponding source text
/// ```
///
/// All spans use half-open byte offsets, not character or line indices. Synthetic
/// nodes may have a default empty span. Sparse maps retain authored declaration
/// and initializer origins; editor reference sites are derived from syntax.
/// Inline module analysis preserves physical source offsets.
///
/// # Panics
///
/// Dense range accessors panic for an invalid index; local-node accessors also check
/// lowering identity and owner. Required parameter/field/variant map lookups panic for an absent key.
/// Plain declaration IDs do not carry a lowering identity: callers must keep the matching map.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// Lowering identity shared with the matching HIR node storage.
    arena: LoweringId,
    /// Current allocation owner; restored when nested declaration lowering returns.
    owner: HirOwner,
    structured_input: bool,
    structured_owner: Option<DefinitionId>,
    structured_origins: HashMap<OriginTarget, DefinitionId>,
    generic_param_spans: Vec<Span>,
    /// Sparse declaration-name sites, separate from full declaration ranges.
    item_name_spans: HashMap<Item, Span>,
    /// Additional full member/use ranges; ordinary items reuse their declaration spans.
    attribute_target_spans: HashMap<AttributeTarget, Span>,
    field_spans: HashMap<FieldId, Span>,
    variant_spans: HashMap<VariantId, Span>,
    function_spans: Vec<Span>,
    const_spans: Vec<Span>,
    static_spans: Vec<Span>,
    /// Full AST initializer ranges, before grouping expressions are erased.
    static_initializer_spans: HashMap<StaticId, Span>,
    module_spans: Vec<Span>,
    trait_spans: Vec<Span>,
    trait_method_spans: HashMap<TraitMethodId, Span>,
    impl_spans: Vec<Span>,
    param_spans: HashMap<ParamId, Span>,
    local_spans: Vec<Span>,
    local_owners: Vec<HirOwner>,
    struct_spans: Vec<Span>,
    native_type_spans: Vec<Span>,
    type_alias_spans: Vec<Span>,
    enum_spans: Vec<Span>,
    block_spans: Vec<Span>,
    block_owners: Vec<HirOwner>,
    expr_spans: Vec<Span>,

    expr_owners: Vec<HirOwner>,
    place_spans: Vec<Span>,

    place_owners: Vec<HirOwner>,
    stmt_spans: Vec<Span>,
    stmt_owners: Vec<HirOwner>,
    pattern_spans: Vec<Span>,

    pattern_owners: Vec<HirOwner>,
    type_spans: Vec<Span>,

    type_owners: Vec<HirOwner>,
}

impl SourceMap {
    pub(crate) fn insert_attribute_target(&mut self, target: AttributeTarget, span: Span) {
        if matches!(target, AttributeTarget::Item(_)) {
            return;
        }
        assert!(
            self.attribute_target_spans.insert(target, span).is_none(),
            "duplicate attribute target"
        );
    }

    /// Returns the full authored declaration/member range for a typed attachment.
    /// IDs must belong to this lowering, just like ordinary `Item` handles.
    ///
    /// # Panics
    ///
    /// Panics if the target was not registered in this source map.
    pub fn attribute_target_span(&self, target: AttributeTarget) -> Span {
        match target {
            AttributeTarget::Item(item) => self.item_span(item),
            _ => self.attribute_target_spans[&target],
        }
    }

    pub(crate) fn push_type_alias(&mut self, span: Span) -> TypeAliasId {
        let id = TypeAliasId::new(self.type_alias_spans.len());
        self.type_alias_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::TypeAlias(id)));
        id
    }

    /// Returns the alias declaration range in the matching lowering.
    pub fn type_alias_span(&self, id: TypeAliasId) -> Span {
        self.type_alias_spans[id.index()]
    }

    pub(crate) fn push_native_type(&mut self, span: Span) -> NativeTypeId {
        let id = NativeTypeId::new(self.native_type_spans.len());
        self.native_type_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::NativeType(id)));
        id
    }

    /// Returns the Native type declaration byte range; see the [ID validity requirements](Self#panics).
    pub fn native_type_span(&self, id: NativeTypeId) -> Span {
        self.native_type_spans[id.index()]
    }

    pub(crate) fn insert_variant(&mut self, id: VariantId, span: Span) {
        self.variant_spans.insert(id, span);
        self.record_origin(OriginTarget::Variant(id));
    }

    /// Returns the enum variant byte range; see the [ID validity requirements](Self#panics).
    pub fn variant_span(&self, id: VariantId) -> Span {
        self.variant_spans[&id]
    }

    /// Installs the owner for future local IDs and returns the previous owner for restoration.
    pub(crate) fn set_owner(&mut self, owner: HirOwner) -> HirOwner {
        mem::replace(&mut self.owner, owner)
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

    /// Returns the lowering identity shared with the matching HIR node storage.
    pub fn arena(&self) -> LoweringId {
        self.arena
    }

    pub(crate) fn insert_generic_param(&mut self, id: GenericParamId, span: Span) {
        assert_eq!(id.index(), self.generic_param_spans.len());
        self.generic_param_spans.push(span);
        self.record_origin(OriginTarget::GenericParam(id));
    }

    /// Returns the generic parameter byte range; see the [ID validity requirements](Self#panics).
    pub fn generic_param_span(&self, id: GenericParamId) -> Span {
        self.generic_param_spans[id.index()]
    }

    pub(crate) fn type_spans(&self) -> &[Span] {
        &self.type_spans
    }

    pub(crate) fn insert_field(&mut self, id: FieldId, span: Span) {
        self.field_spans.insert(id, span);
        self.record_origin(OriginTarget::Field(id));
    }

    /// Returns the struct field byte range; see the [ID validity requirements](Self#panics).
    pub fn field_span(&self, id: FieldId) -> Span {
        self.field_spans[&id]
    }

    /// Returns the full declaration range selected by its local item handle.
    pub fn item_span(&self, item: Item) -> Span {
        match item {
            Item::TypeAlias(id) => self.type_alias_span(id),
            Item::NativeType(id) => self.native_type_span(id),
            Item::Function(id) => self.function_span(id),
            Item::Const(id) => self.const_span(id),
            Item::Static(id) => self.static_span(id),
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

    pub(crate) fn insert_function(&mut self, id: FunctionId, span: Span) {
        assert_eq!(id.index(), self.function_spans.len());
        self.function_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::Function(id)));
    }

    pub(crate) fn push_static(&mut self, span: Span) -> StaticId {
        let id = StaticId::new(self.static_spans.len());
        self.static_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::Static(id)));
        id
    }

    pub(crate) fn push_const(&mut self, span: Span) -> ConstId {
        let id = ConstId::new(self.const_spans.len());
        self.const_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::Const(id)));
        id
    }

    pub(crate) fn push_module(&mut self, span: Span) -> ModuleId {
        let id = ModuleId::new(self.module_spans.len());
        self.module_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::Module(id)));
        id
    }

    pub(crate) fn push_trait(&mut self, span: Span) -> TraitId {
        let id = TraitId::new(self.trait_spans.len());
        self.trait_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::Trait(id)));
        id
    }

    pub(crate) fn insert_trait_method(&mut self, id: TraitMethodId, span: Span) {
        assert_eq!(id.arena(), self.arena);
        assert!(self.trait_method_spans.insert(id, span).is_none());
        self.record_origin(OriginTarget::TraitMethod(id));
    }

    pub(crate) fn push_impl(&mut self, span: Span) -> ImplId {
        let id = ImplId::new(self.impl_spans.len());
        self.impl_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::Impl(id)));
        id
    }

    pub(crate) fn insert_param(&mut self, id: ParamId, span: Span) {
        assert_eq!(id.arena(), self.arena, "foreign HIR parameter registration");
        assert_eq!(id.owner(), self.owner, "foreign HIR parameter owner");
        assert!(matches!(id.owner(), HirOwner::Body(BodyOwner::Function(_))));
        assert!(
            self.param_spans.insert(id, span).is_none(),
            "duplicate HIR parameter"
        );
        self.record_origin(OriginTarget::Param(id));
    }

    pub(crate) fn owner(&self) -> HirOwner {
        self.owner
    }

    pub(crate) fn insert_local(&mut self, id: LocalId, span: Span) {
        assert_eq!(id.arena(), self.arena);
        assert_eq!(id.index(), self.local_spans.len());
        self.local_spans.push(span);
        self.local_owners.push(id.owner());
        self.record_origin(OriginTarget::Local(id));
    }

    pub(crate) fn push_struct(&mut self, span: Span) -> StructId {
        let id = StructId::new(self.struct_spans.len());
        self.struct_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::Struct(id)));
        id
    }

    pub(crate) fn push_enum(&mut self, span: Span) -> EnumId {
        let id = EnumId::new(self.enum_spans.len());
        self.enum_spans.push(span);
        self.record_origin(OriginTarget::Item(Item::Enum(id)));
        id
    }

    pub(crate) fn push_block(&mut self, span: Span) -> BlockId {
        let id = BlockId::new(self.arena, self.owner, self.block_spans.len());
        self.block_spans.push(span);
        self.block_owners.push(self.owner);
        self.record_origin(OriginTarget::Block(id));
        id
    }

    pub(crate) fn push_expr(&mut self, span: Span) -> ExprId {
        let id = ExprId::new(self.arena, self.owner, self.expr_spans.len());
        self.expr_spans.push(span);
        self.expr_owners.push(self.owner);
        self.record_origin(OriginTarget::Expr(id));
        id
    }

    pub(crate) fn insert_item_name(&mut self, item: Item, span: Span) {
        self.item_name_spans.insert(item, span);
    }

    /// Returns a recorded declaration-name range; absent for nameless/recovered items.
    pub fn item_name_span(&self, item: Item) -> Option<Span> {
        self.item_name_spans.get(&item).copied()
    }

    /// Returns the name range when recorded, otherwise the full declaration range.
    pub fn item_declaration_span(&self, item: Item) -> Span {
        self.item_name_span(item)
            .unwrap_or_else(|| self.item_span(item))
    }

    pub(crate) fn push_place(&mut self, span: Span) -> PlaceId {
        let id = PlaceId::new(self.arena, self.owner, self.place_spans.len());
        self.place_spans.push(span);
        self.place_owners.push(self.owner);
        self.record_origin(OriginTarget::Place(id));
        id
    }

    pub(crate) fn push_stmt(&mut self, span: Span) -> StmtId {
        let id = StmtId::new(self.arena, self.owner, self.stmt_spans.len());
        self.stmt_spans.push(span);
        self.stmt_owners.push(self.owner);
        self.record_origin(OriginTarget::Stmt(id));
        id
    }

    pub(crate) fn push_pattern(&mut self, span: Span) -> PatternId {
        let id = PatternId::new(self.arena, self.owner, self.pattern_spans.len());
        self.pattern_spans.push(span);
        self.pattern_owners.push(self.owner);
        self.record_origin(OriginTarget::Pattern(id));
        id
    }

    pub(crate) fn push_type(&mut self, span: Span) -> TypeRefId {
        let id = TypeRefId::new(self.arena, self.owner, self.type_spans.len());
        self.type_spans.push(span);
        self.type_owners.push(self.owner);
        self.record_origin(OriginTarget::Type(id));
        id
    }

    /// Returns the function byte range; see the [ID validity requirements](Self#panics).
    pub fn function_span(&self, id: FunctionId) -> Span {
        self.function_spans[id.index()]
    }

    /// Returns the child module header byte range; see the [ID validity requirements](Self#panics).
    pub fn module_span(&self, id: ModuleId) -> Span {
        self.module_spans[id.index()]
    }

    /// Returns the trait byte range; see the [ID validity requirements](Self#panics).
    pub fn trait_span(&self, id: TraitId) -> Span {
        self.trait_spans[id.index()]
    }

    /// Returns the trait method byte range; see the [ID validity requirements](Self#panics).
    pub fn trait_method_span(&self, id: TraitMethodId) -> Span {
        self.trait_method_spans[&id]
    }

    /// Returns the implementation block byte range; see the [ID validity requirements](Self#panics).
    pub fn impl_span(&self, id: ImplId) -> Span {
        self.impl_spans[id.index()]
    }

    /// Returns the constant byte range; see the [ID validity requirements](Self#panics).
    pub fn const_span(&self, id: ConstId) -> Span {
        self.const_spans[id.index()]
    }

    /// Returns the declaration span for a static in this lowering.
    pub fn static_span(&self, id: StaticId) -> Span {
        self.static_spans[id.index()]
    }

    /// Returns the complete authored initializer, including outer parentheses.
    /// Missing recovery initializers have no range. Interpret it in the matching source.
    pub fn static_initializer_span(&self, id: StaticId) -> Option<Span> {
        self.static_initializer_spans.get(&id).copied()
    }

    pub(crate) fn insert_static_initializer(&mut self, id: StaticId, span: Span) {
        self.static_initializer_spans.insert(id, span);
    }

    /// Returns the parameter byte range; see the [ID validity requirements](Self#panics).
    pub fn param_span(&self, id: ParamId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        *self
            .param_spans
            .get(&id)
            .expect("unregistered HIR parameter source range")
    }

    /// Returns the local binding byte range; see the [ID validity requirements](Self#panics).
    pub fn local_span(&self, id: LocalId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.local_owners[id.index()],
            "foreign HIR body source range"
        );
        self.local_spans[id.index()]
    }

    /// Returns the struct declaration byte range; see the [ID validity requirements](Self#panics).
    pub fn struct_span(&self, id: StructId) -> Span {
        self.struct_spans[id.index()]
    }

    /// Returns the enum declaration byte range; see the [ID validity requirements](Self#panics).
    pub fn enum_span(&self, id: EnumId) -> Span {
        self.enum_spans[id.index()]
    }

    /// Returns the block byte range; see the [ID validity requirements](Self#panics).
    pub fn block_span(&self, id: BlockId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.block_owners[id.index()],
            "foreign HIR body source range"
        );
        self.block_spans[id.index()]
    }

    /// Returns the expression byte range; see the [ID validity requirements](Self#panics).
    pub fn expr_span(&self, id: ExprId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.expr_owners[id.index()],
            "foreign HIR body source range"
        );
        self.expr_spans[id.index()]
    }

    /// Returns the assignment place byte range; see the [ID validity requirements](Self#panics).
    pub fn place_span(&self, id: PlaceId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.place_owners[id.index()],
            "foreign HIR body source range"
        );
        self.place_spans[id.index()]
    }

    /// Returns the statement byte range; see the [ID validity requirements](Self#panics).
    pub fn stmt_span(&self, id: StmtId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.stmt_owners[id.index()],
            "foreign HIR body source range"
        );
        self.stmt_spans[id.index()]
    }

    /// Returns the pattern byte range; see the [ID validity requirements](Self#panics).
    pub fn pattern_span(&self, id: PatternId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.pattern_owners[id.index()],
            "foreign HIR body source range"
        );
        self.pattern_spans[id.index()]
    }

    /// Returns the type syntax byte range; see the [ID validity requirements](Self#panics).
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
