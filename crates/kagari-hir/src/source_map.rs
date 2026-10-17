//! Allocation-aligned byte ranges and specialized navigation sites for lowered HIR.

use crate::hir::{
    ids::{
        BlockId, ConstId, EnumId, ExprId, FieldId, FunctionId, GenericParamId, HirArenaId,
        HirOwner, ImplId, LocalId, ModuleId, OpaqueTypeId, ParamId, PatternId, PlaceId, StmtId,
        StructId, TraitId, TraitMethodId, TypeRefId, VariantId,
    },
    item::Item,
};
use kagari_common::span::Span;
use std::{collections::HashMap, mem};

/// Source byte ranges and ID allocation state for one [`crate::lower::LoweredModule`].
///
/// This owns ranges, not source text. Interpret them against `LoweredModule.source`.
/// The lowerer appends spans and payload rows together; their indices must agree.
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
/// nodes may have a default empty span. Full node ranges can include syntax trivia. Sparse maps record specialized navigation
/// sites such as a terminal member name or path segment; absence does not imply a
/// missing expression. Inline module analysis preserves physical source offsets.
///
/// # Panics
///
/// Dense range accessors panic for an invalid index; local-node accessors also check
/// arena and owner. Required field/variant map lookups panic for an absent key.
/// Plain declaration IDs do not carry an arena: callers must keep the matching map.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// Arena identity shared with the matching HIR node storage.
    arena: HirArenaId,
    /// Current allocation owner; restored when nested declaration lowering returns.
    owner: HirOwner,
    generic_param_spans: Vec<Span>,
    /// Sparse declaration-name sites, separate from full declaration ranges.
    item_name_spans: HashMap<Item, Span>,
    field_spans: HashMap<FieldId, Span>,
    variant_spans: HashMap<VariantId, Span>,
    function_spans: Vec<Span>,
    const_spans: Vec<Span>,
    module_spans: Vec<Span>,
    trait_spans: Vec<Span>,
    trait_method_spans: Vec<Span>,
    impl_spans: Vec<Span>,
    param_spans: Vec<Span>,
    param_owners: Vec<HirOwner>,
    local_spans: Vec<Span>,
    local_owners: Vec<HirOwner>,
    struct_spans: Vec<Span>,
    opaque_type_spans: Vec<Span>,
    enum_spans: Vec<Span>,
    block_spans: Vec<Span>,
    block_owners: Vec<HirOwner>,
    expr_spans: Vec<Span>,
    /// Reference/member-name sites used by navigation and diagnostics.
    expr_reference_spans: HashMap<ExprId, Span>,
    /// Ordered joined-path prefixes and their physical segment sites, keyed by expression.
    expr_paths: HashMap<ExprId, Vec<(String, Span)>>,
    /// Joined-path prefix sites keyed by the flattened import leaf slot.
    import_paths: HashMap<usize, Vec<(String, Span)>>,
    /// Explicit constructor/type-owner sites, independent of the expression's body owner.
    expr_owner_spans: HashMap<ExprId, Span>,
    /// Constructor field-name sites in field order, retaining absent synthetic sites.
    struct_field_spans: HashMap<ExprId, Vec<Option<Span>>>,
    expr_owners: Vec<HirOwner>,
    place_spans: Vec<Span>,
    place_member_spans: HashMap<PlaceId, Span>,
    place_owners: Vec<HirOwner>,
    stmt_spans: Vec<Span>,
    stmt_owners: Vec<HirOwner>,
    pattern_spans: Vec<Span>,
    pattern_reference_spans: HashMap<PatternId, Span>,
    pattern_owners: Vec<HirOwner>,
    type_spans: Vec<Span>,
    type_name_spans: HashMap<TypeRefId, Span>,
    /// Joined type-path prefixes and their physical segment sites.
    type_paths: HashMap<TypeRefId, Vec<(String, Span)>>,
    /// Terminal type/member sites, separate from the full type path.
    type_terminal_spans: HashMap<TypeRefId, Span>,
    type_owners: Vec<HirOwner>,
}

impl SourceMap {
    pub(crate) fn insert_type_path(&mut self, id: TypeRefId, path: Vec<(String, Span)>) {
        self.type_paths.insert(id, path);
    }

    pub(crate) fn type_path(&self, id: TypeRefId) -> &[(String, Span)] {
        self.type_paths.get(&id).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn insert_expr_path(&mut self, id: ExprId, path: Vec<(String, Span)>) {
        self.expr_paths.insert(id, path);
    }

    pub(crate) fn expr_path(&self, id: ExprId) -> &[(String, Span)] {
        self.expr_paths.get(&id).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn insert_import_path(&mut self, slot: usize, path: Vec<(String, Span)>) {
        self.import_paths.insert(slot, path);
    }

    pub(crate) fn import_path(&self, slot: usize) -> &[(String, Span)] {
        self.import_paths.get(&slot).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn push_opaque_type(&mut self, span: Span) -> OpaqueTypeId {
        let id = OpaqueTypeId::new(self.opaque_type_spans.len());
        self.opaque_type_spans.push(span);
        id
    }

    /// Returns the opaque type declaration byte range; see the [ID validity requirements](Self#panics).
    pub fn opaque_type_span(&self, id: OpaqueTypeId) -> Span {
        self.opaque_type_spans[id.index()]
    }

    pub(crate) fn insert_variant(&mut self, id: VariantId, span: Span) {
        self.variant_spans.insert(id, span);
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

    /// Returns the arena shared with the matching HIR node storage.
    pub fn arena(&self) -> HirArenaId {
        self.arena
    }

    pub(crate) fn push_generic_param(&mut self, span: Span) -> GenericParamId {
        let id = GenericParamId::new(self.generic_param_spans.len());
        self.generic_param_spans.push(span);
        id
    }

    /// Returns the generic parameter byte range; see the [ID validity requirements](Self#panics).
    pub fn generic_param_span(&self, id: GenericParamId) -> Span {
        self.generic_param_spans[id.index()]
    }

    pub(crate) fn type_spans(&self) -> &[Span] {
        &self.type_spans
    }

    pub(crate) fn insert_type_terminal(&mut self, id: TypeRefId, span: Span) {
        self.type_terminal_spans.insert(id, span);
    }

    /// Returns the terminal associated-type member range, if recorded.
    pub fn type_terminal_span(&self, id: TypeRefId) -> Option<Span> {
        self.type_terminal_spans.get(&id).copied()
    }

    pub(crate) fn insert_pattern_reference(&mut self, id: PatternId, span: Span) {
        self.pattern_reference_spans.insert(id, span);
    }

    /// Returns a referenced pattern path/name range, if recorded.
    pub fn pattern_reference_span(&self, id: PatternId) -> Option<Span> {
        self.pattern_reference_spans.get(&id).copied()
    }

    pub(crate) fn insert_type_name(&mut self, id: TypeRefId, span: Span) {
        self.type_name_spans.insert(id, span);
    }

    /// Returns the type-name range, if lowering recorded a named type site.
    pub fn type_name_span(&self, id: TypeRefId) -> Option<Span> {
        self.type_name_spans.get(&id).copied()
    }

    pub(crate) fn insert_field(&mut self, id: FieldId, span: Span) {
        self.field_spans.insert(id, span);
    }

    /// Returns the struct field byte range; see the [ID validity requirements](Self#panics).
    pub fn field_span(&self, id: FieldId) -> Span {
        self.field_spans[&id]
    }

    /// Returns the full declaration range selected by its local item handle.
    pub fn item_span(&self, item: Item) -> Span {
        match item {
            Item::OpaqueType(id) => self.opaque_type_span(id),
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

    pub(crate) fn insert_expr_reference(&mut self, id: ExprId, span: Span) {
        self.expr_reference_spans.insert(id, span);
    }

    /// Returns the expression reference/member range, if recorded.
    pub fn expr_reference_span(&self, id: ExprId) -> Option<Span> {
        self.expr_reference_spans.get(&id).copied()
    }

    pub(crate) fn insert_expr_owner(&mut self, id: ExprId, span: Span) {
        self.expr_owner_spans.insert(id, span);
    }

    /// Returns the explicit type/constructor-owner range, if recorded.
    pub fn expr_owner_span(&self, id: ExprId) -> Option<Span> {
        self.expr_owner_spans.get(&id).copied()
    }

    pub(crate) fn insert_struct_fields(&mut self, id: ExprId, spans: Vec<Option<Span>>) {
        self.struct_field_spans.insert(id, spans);
    }

    /// Returns constructor field-name sites in field order; individual synthetic sites may be absent.
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

    /// Returns the assigned member-name range, if recorded.
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
        self.trait_method_spans[id.index()]
    }

    /// Returns the implementation block byte range; see the [ID validity requirements](Self#panics).
    pub fn impl_span(&self, id: ImplId) -> Span {
        self.impl_spans[id.index()]
    }

    /// Returns the constant byte range; see the [ID validity requirements](Self#panics).
    pub fn const_span(&self, id: ConstId) -> Span {
        self.const_spans[id.index()]
    }

    /// Returns the parameter byte range; see the [ID validity requirements](Self#panics).
    pub fn param_span(&self, id: ParamId) -> Span {
        assert_eq!(id.arena(), self.arena, "foreign HIR source range");
        assert_eq!(
            id.owner(),
            self.param_owners[id.index()],
            "foreign HIR body source range"
        );
        self.param_spans[id.index()]
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
