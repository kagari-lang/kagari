//! Nominal struct, enum and native-backed type declarations; no runtime object storage.

use crate::hir::{
    ids::{EnumId, FieldId, ImplId, MethodId, OpaqueTypeId, StructId, TypeRefId, VariantId},
    item::behavior::{GenericParam, TraitBound, TraitRef},
    writeability::Writeability,
};
use kagari_types::visibility::Visibility;

/// A type surface whose storage/identity must come from an installed native provider.
///
/// ```text
/// pub type Handle<T: Display>: Marker where T: Copy;
/// OpaqueType {
///     id: o, visibility: Public, name: "Handle",
///     generic_params: [GenericParam { id: g, name: "T", bounds: [display_ref] }],
///     bounds: [TraitBound { target: "T", target_ref: t, traits: [copy_ref] }],
///     trait_bounds: [marker_ref], definition: None,
/// }
/// o -> Module.opaque_types[o.index()]
/// t / each TraitRef.ty -> Body.type_ref -> target / trait application syntax
/// ```
///
/// This illustrates the offline/native declaration surface accepted by
/// `kagari_syntax::parser::parse_declarations`; ordinary script parsing rejects
/// top-level `type` declarations. It is not a script-created storage definition
/// or ordinary type alias. A matching installed declaration is required. Omitted generics/where/output bounds leave their buffers empty;
/// absence of `pub` selects private visibility. An `= Type` suffix, if parsed,
/// is retained as `definition: Some(type_id)` for later validation, not automatic
/// alias expansion. Native registration and signature checking own acceptance.
#[derive(Debug, Clone)]
pub struct OpaqueType {
    /// Slot in `Module.opaque_types`.
    pub id: OpaqueTypeId,
    /// Declared visibility.
    pub visibility: Visibility,
    /// Declared nominal type name.
    pub name: String,
    /// Generic binders in declaration order.
    pub generic_params: Vec<GenericParam>,
    /// Where-clause constraints before semantic resolution.
    pub bounds: Vec<TraitBound>,
    /// Declared trait requirements on the type surface.
    pub trait_bounds: Vec<TraitRef>,
    /// Optional source type definition retained for validation.
    pub definition: Option<TypeRefId>,
}

/// A nominal struct declaration with inline field records, not a runtime object.
///
/// ```text
/// pub struct Point<T> { pub var x: T, val label: String }
/// Struct {
///     id: s, visibility: Public, name: "Point",
///     generic_params: [GenericParam { id: g, name: "T", bounds: [] }],
///     fields: [x_field, label_field], methods: [], impls: [],
/// }
/// x_field = Field { id: x, visibility: Public, writeability: Var, name: "x", ty: t }
/// x = FieldId { arena, owner: s, slot: 0 }
/// Module.field(x) -> Module.structs[s.index()].fields[0]
/// t -> Body.type_ref -> Named("T")
/// label_field has slot 1, Private visibility, Val writeability and String type syntax
/// ```
///
/// `s` is allocated from the declaration/source-map slots. Fields retain source
/// order; `struct Empty {}` has no fields, and omitted generics leave no binders.
/// Field types and generic requirements are unresolved until signature checking.
/// Current lowering leaves `methods` and `impls` empty, including when a separate
/// `impl Point<i32> { ... }` exists; active method ownership lives in Module.impls.
/// These vectors must not be read as a complete list of associated members.
#[derive(Debug, Clone)]
pub struct Struct {
    /// Slot in `Module.structs`.
    pub id: StructId,
    /// Declared visibility.
    pub visibility: Visibility,
    /// Nominal struct name.
    pub name: String,
    /// Generic binders in source order.
    pub generic_params: Vec<GenericParam>,
    /// Inline fields addressed by arena, struct owner and slot.
    pub fields: FieldBuffer,
    /// Unpopulated method-registry links; current members are stored in trait/impl records.
    pub methods: Vec<MethodId>,
    /// Unpopulated impl links; inspect `Module.impls` and checked lookup facts instead.
    pub impls: Vec<ImplId>,
}

/// A field declaration stored inline in its struct, distinct from a field initializer.
///
/// ```text
/// struct Point { pub var x: i32 }
/// Field { id: f, visibility: Public, writeability: Var, name: "x", ty: t }
/// f = FieldId { arena, owner: point_id, slot: 0 }
/// t -> Body.type_ref -> Named("i32")
/// f -> Module.field(f); SourceMap.field_span(f) records the name site
/// ```
///
/// `pub`, `var`, `x` and `i32` supply visibility, writeability, name and type syntax;
/// `id` is synthesized from the enclosing struct and member slot. `val` instead
/// selects `Val`, and omitted visibility selects `Private`. A constructor's
/// `Point { x: 1 }` uses FieldInit with an expression value, not this declaration.
/// Checking determines field layout/access; missing type syntax becomes a placeholder.
#[derive(Debug, Clone)]
pub struct Field {
    /// Arena/struct/slot identity used by `Module::field`.
    pub id: FieldId,
    /// Visibility required to select this field.
    pub visibility: Visibility,
    /// Declared field assignment policy.
    pub writeability: Writeability,
    /// Field spelling within its struct.
    pub name: String,
    /// Unresolved field type syntax.
    pub ty: TypeRefId,
}

/// A nominal enum declaration with inline positional variant payload types.
///
/// ```text
/// pub enum Choice<T> { None, Some(T) }
/// Enum {
///     id: e, visibility: Public, name: "Choice",
///     generic_params: [GenericParam { id: g, name: "T", bounds: [] }],
///     variants: [Variant { id: none_id, name: "None", payload: [] },
///                Variant { id: some_id, name: "Some", payload: [t] }],
///     methods: [], impls: [],
/// }
/// some_id = VariantId { arena, owner: e, slot: 1 }
/// Module.variant(some_id) -> enums[e.index()].variants[1]
/// t -> Body.type_ref -> Named("T")
/// ```
///
/// `e` is an allocated declaration slot; variant IDs carry their enum owner and
/// source-order slot. Omitted generics/visibility give empty binders/Private.
/// Current lowering leaves `methods`/`impls` empty; separate impl records carry
/// active methods. Signature checking builds variant/type facts, and checked
/// aggregate/executable metadata determines representation, not this source row.
#[derive(Debug, Clone)]
pub struct Enum {
    /// Slot in `Module.enums`.
    pub id: EnumId,
    /// Declared visibility.
    pub visibility: Visibility,
    /// Nominal enum name.
    pub name: String,
    /// Generic binders in source order.
    pub generic_params: Vec<GenericParam>,
    /// Inline variants addressed by arena, enum owner and slot.
    pub variants: VariantBuffer,
    /// Unpopulated method-registry links; current members are stored in trait/impl records.
    pub methods: Vec<MethodId>,
    /// Unpopulated impl links; inspect `Module.impls` and checked lookup facts instead.
    pub impls: Vec<ImplId>,
}

/// One enum variant declaration, stored inline in Enum.variants.
///
/// ```text
/// enum Event { Idle, Pair(i32, String) }
/// Idle -> Variant { id: idle_id, name: "Idle", payload: [] }
/// Pair -> Variant { id: pair_id, name: "Pair", payload: [i32_type, string_type] }
/// pair_id = VariantId { arena, owner: event_id, slot: 1 }
/// each payload ID -> Body.type_ref -> declared type syntax
/// ```
///
/// `name` and payload type order come from source; `id` is allocated from the
/// enum/slot context. `payload` holds types, not constructor argument expressions
/// or runtime values. `Event::Pair(1, "a")` is instead a Call expression, while
/// `Event::Pair(x, _)` in a match is an EnumVariant pattern.
#[derive(Debug, Clone)]
pub struct Variant {
    /// Arena/enum/slot identity used by `Module::variant`.
    pub id: VariantId,
    /// Variant name within its enum.
    pub name: String,
    /// Payload type syntax in positional order; empty for a unit variant.
    pub payload: Vec<TypeRefId>,
}

/// Ordered `Vec<Struct>` storage for the owning declaration records.
pub type StructBuffer = Vec<Struct>;
/// Ordered `Vec<Field>` storage for the owning declaration records.
pub type FieldBuffer = Vec<Field>;
/// Ordered `Vec<Enum>` storage for the owning declaration records.
pub type EnumBuffer = Vec<Enum>;
/// Ordered `Vec<Variant>` storage for the owning declaration records.
pub type VariantBuffer = Vec<Variant>;
