//! Nominal struct, enum and native-backed type declarations; no runtime object storage.

use crate::hir::{
    ids::{EnumId, FieldId, ImplId, MethodId, OpaqueTypeId, StructId, TypeRefId, VariantId},
    item::behavior::{GenericParam, TraitBound, TraitRef},
    writeability::Writeability,
};
use kagari_types::visibility::Visibility;

/// A declaration whose storage is supplied by an installed native provider.
/// Its representation is checked separately from its ordinary generic syntax.
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

/// A nominal struct declaration with inline field records.
///
/// ```text
/// struct Point { val x: i32 }
/// Module.structs[s.index()] -> Struct { fields: [Field { id: f, ty: t, ... }], ... }
/// f = FieldId { arena, owner: s, slot: 0 }
/// Module.field(f) -> structs[s.index()].fields[0]
/// t -> Body.type_ref(t) -> Named("i32")
/// ```
///
/// This is declaration syntax, not an object allocation or physical field layout.
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
    /// Associated method handles into `Module.methods`.
    pub methods: Vec<MethodId>,
    /// Associated implementation handles into `Module.impls`.
    pub impls: Vec<ImplId>,
}

/// A struct field stored inside its owner's `fields` vector, not in a global field arena.
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

/// A nominal enum declaration with inline variant payload types.
///
/// ```text
/// enum Choice { None, Some(i32) }
/// Module.enums[e.index()] -> Enum { variants: [None, Some], ... }  // names abbreviated
/// VariantId { arena, owner: e, slot: 1 } -> Variant { name: "Some", payload: [t], ... }
/// t -> Body.type_ref(t) -> Named("i32")
/// ```
///
/// Variant IDs locate declarations; the eventual value representation belongs to
/// checked aggregate/executable metadata.
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
    /// Associated method handles into `Module.methods`.
    pub methods: Vec<MethodId>,
    /// Associated implementation handles into `Module.impls`.
    pub impls: Vec<ImplId>,
}

/// One enum variant and its ordered payload type syntax.
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
