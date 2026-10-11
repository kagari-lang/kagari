//! Intrinsic Array element context and separately installed List indexing roles.
use crate::{
    declarations::Declarations,
    types::{NominalType, TypeId},
};
use kagari_types::collection::CollectionAccess;

/// Borrows the first argument of a registered List interface; unrelated/malformed types return `None`.
pub fn list_item<'a>(ty: &'a TypeId, declarations: &Declarations) -> Option<&'a TypeId> {
    let TypeId::Trait(interface) = ty else {
        return None;
    };
    declarations
        .array_interfaces
        .values()
        .any(|id| *id == interface.declaration)
        .then(|| interface.arguments.first())
        .flatten()
}

/// Checks whether a trait application names the installed MutableList interface.
pub fn writable_list(ty: &TypeId, declarations: &Declarations) -> bool {
    matches!(ty, TypeId::Trait(interface) if declarations.array_interfaces.get(&CollectionAccess::Mutable) == Some(&interface.declaration))
}

pub(crate) fn element_context(ty: &TypeId) -> Option<&TypeId> {
    if let TypeId::Array(item) = ty {
        Some(item)
    } else {
        None
    }
}

/// Builds the installed List/MutableList interface for an element type; returns `None` if missing.
pub fn list_interface(
    item: TypeId,
    writable: bool,
    declarations: &Declarations,
) -> Option<NominalType> {
    let access = if writable {
        CollectionAccess::Mutable
    } else {
        CollectionAccess::ReadOnly
    };
    Some(NominalType {
        declaration: declarations.array_interfaces.get(&access)?.clone(),
        arguments: vec![item],
        associated_types: Default::default(),
    })
}
