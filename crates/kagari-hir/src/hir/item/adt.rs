//! Nominal struct, enum and native-backed type declarations; no runtime object storage.

use crate::hir::{
    ids::{EnumId, FieldId, ImplId, NativeTypeId, StructId, TypeRefId, VariantId},
    item::behavior::{GenericParam, TraitBound, TraitRef},
    writeability::Writeability,
};
use kagari_types::visibility::Visibility;

/// A Native nominal interface, independent of its installed storage representation.
///
/// ```text
/// pub native type Handle<T: Display> { pub var value: T }
/// NativeTypeDeclaration {
///     id: n, visibility: Public, name: "Handle",
///     generic_params: [GenericParam { id: g, name: "T", bounds: [display_ref] }],
///     bounds: [], trait_bounds: [],
///     fields: [Field { id: f, visibility: Public, writeability: Var, name: "value", ty: t }],
/// }
/// n -> Module.native_types[n.index()]
/// f = FieldId { arena, owner: FieldOwner::NativeType(n), slot: 0 }
/// t / each TraitRef.ty -> Body.type_ref -> field / trait application syntax
/// ```
///
/// Fields use ordinary visibility, type and var/val checking, not Rust layout or
/// executable adapters. A semicolon declaration has no fields. Omitted generic
/// and bound lists remain empty; omitted visibility selects private access.
/// Declaration-only analysis needs no installed provider. Executable admission
/// separately requires matching representation facts; native field execution is
/// unsupported until runtime adapters exist. Transparent `type Alias = Target`
/// declarations have a separate alias owner and create no Native identity.
#[derive(Debug, Clone)]
pub struct NativeTypeDeclaration {
    /// Slot in `Module.native_types`.
    pub id: NativeTypeId,
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
    /// Fields in declaration order, using the same interface rules as struct fields.
    /// These records describe no Rust offsets or runtime access adapters.
    pub fields: Vec<Field>,
}

/// A nominal struct declaration with inline field records, not a runtime object.
///
/// ```text
/// pub struct Point<T> { pub var x: T, val label: String }
/// Struct {
///     id: s, visibility: Public, name: "Point",
///     generic_params: [GenericParam { id: g, name: "T", bounds: [] }],
///     fields: [x_field, label_field], impls: [],
/// }
/// x_field = Field { id: x, visibility: Public, writeability: Var, name: "x", ty: t }
/// x = FieldId { arena, owner: FieldOwner::Struct(s), slot: 0 }
/// Module.field(x) -> Module.structs[s.index()].fields[0]
/// t -> Body.type_ref -> Named("T")
/// label_field has slot 1, Private visibility, Val writeability and String type syntax
/// ```
///
/// `s` is allocated from the declaration/source-map slots. Fields retain source
/// order; `struct Empty {}` has no fields, and omitted generics leave no binders.
/// Field types and generic requirements are unresolved until signature checking.
/// Current lowering leaves `impls` empty, including when a separate
/// `impl Point<i32> { ... }` exists; active method ownership lives in Module.impls.
/// This vector must not be read as a complete list of associated implementations.
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
    pub fields: Vec<Field>,
    /// Unpopulated impl links; inspect `Module.impls` and checked lookup facts instead.
    pub impls: Vec<ImplId>,
}

/// A field declaration stored inline in its struct or Native type, not a field initializer.
///
/// ```text
/// struct Point { pub var x: i32 }
/// Field { id: f, visibility: Public, writeability: Var, name: "x", ty: t }
/// f = FieldId { arena, owner: FieldOwner::Struct(point_id), slot: 0 }
/// t -> Body.type_ref -> Named("i32")
/// f -> Module.field(f); SourceMap.field_span(f) records the name site
/// ```
///
/// `pub`, `var`, `x` and `i32` supply visibility, writeability, name and type syntax;
/// `id` is synthesized from the enclosing declaration and member slot. `val` instead
/// selects `Val`, and omitted visibility selects `Private`. A constructor's
/// `Point { x: 1 }` uses FieldInit with an expression value, not this declaration.
/// Checking determines field layout/access; missing type syntax becomes a placeholder.
#[derive(Debug, Clone)]
pub struct Field {
    /// Lowering/declaration/slot identity used by `Module::field`.
    pub id: FieldId,
    /// Visibility required to select this field.
    pub visibility: Visibility,
    /// Declared field assignment policy.
    pub writeability: Writeability,
    /// Field spelling within its owning declaration.
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
///     impls: [],
/// }
/// some_id = VariantId { arena, owner: e, slot: 1 }
/// Module.variant(some_id) -> enums[e.index()].variants[1]
/// t -> Body.type_ref -> Named("T")
/// ```
///
/// `e` is an allocated declaration slot; variant IDs carry their enum owner and
/// source-order slot. Omitted generics/visibility give empty binders/Private.
/// Current lowering leaves `impls` empty; separate impl records carry
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
    pub variants: Vec<Variant>,
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
