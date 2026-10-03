//! The bounded legacy bridge for array context and indexed list assignment.
//! General collection selection must use checked declarations/implementations.
use crate::types::{NominalType, TypeId};
use kagari_contract::library;

pub fn list_item(ty: &TypeId) -> Option<&TypeId> {
    let TypeId::Trait(interface) = ty else {
        return None;
    };
    (interface.declaration == library::trait_id("List")
        || interface.declaration == library::trait_id("MutableList"))
    .then(|| interface.arguments.first())
    .flatten()
}

pub fn writable_list(ty: &TypeId) -> bool {
    matches!(ty, TypeId::Trait(interface) if interface.declaration == library::trait_id("MutableList"))
}

pub(crate) fn element_context(ty: &TypeId) -> Option<&TypeId> {
    if let TypeId::Array(item, _) = ty {
        Some(item)
    } else {
        list_item(ty)
    }
}

/// The declared List context selected by array syntax and legacy assignment.
pub fn list_interface(item: TypeId, writable: bool) -> NominalType {
    NominalType {
        declaration: library::trait_id(if writable { "MutableList" } else { "List" }),
        arguments: vec![item],
        associated_types: Default::default(),
    }
}
