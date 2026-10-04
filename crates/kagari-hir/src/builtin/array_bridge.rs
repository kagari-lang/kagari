//! Array context and indexing use interfaces supplied by installed declarations.
use crate::{
    declarations::Declarations,
    types::{NominalType, TypeId},
};
use kagari_types::collection::CollectionAccess;

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

pub fn writable_list(ty: &TypeId, declarations: &Declarations) -> bool {
    matches!(ty, TypeId::Trait(interface) if declarations.array_interfaces.get(&CollectionAccess::Mutable) == Some(&interface.declaration))
}

pub(crate) fn element_context<'a>(
    ty: &'a TypeId,
    declarations: &Declarations,
) -> Option<&'a TypeId> {
    if let TypeId::Array(item, _) = ty {
        Some(item)
    } else {
        list_item(ty, declarations)
    }
}

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
