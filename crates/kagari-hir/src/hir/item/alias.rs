//! Transparent script type declarations, separate from native opaque types.

use crate::hir::{
    ids::{TypeAliasId, TypeRefId},
    item::behavior::{GenericParam, TraitBound},
};
use kagari_types::visibility::Visibility;

/// A source alias whose target is resolved in its declaring module.
///
/// ```text
/// pub type Values<T> = [T];
/// TypeAlias { id: a, visibility: Public, name: "Values",
///             generic_params: [T], bounds: [], target: Some(t) }
/// a -> Module.type_aliases[a.index()]
/// t -> Body.type_ref -> Array(element) -> Named("T")
/// ```
///
/// The ID supports navigation and generic binder ownership only. Checked types
/// contain the substituted target, never an alias nominal identity. A missing
/// target is retained for malformed-source recovery and rejected by checking.
#[derive(Debug, Clone)]
pub struct TypeAlias {
    /// Local slot in `Module.type_aliases`.
    pub id: TypeAliasId,
    /// Declared exposure, which must not widen target visibility.
    pub visibility: Visibility,
    /// Source declaration spelling.
    pub name: String,
    /// Ordered alias-owned generic binders.
    pub generic_params: Vec<GenericParam>,
    /// Where-clause requirements checked alongside inline parameter bounds.
    pub bounds: Vec<TraitBound>,
    /// Written target type, absent only during recovery.
    pub target: Option<TypeRefId>,
}
