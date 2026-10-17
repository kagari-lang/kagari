//! Intrinsic fixed-length Array element context, independent of library interfaces.
use crate::types::TypeId;

pub(crate) fn element_context(ty: &TypeId) -> Option<&TypeId> {
    if let TypeId::Array(item) = ty {
        Some(item)
    } else {
        None
    }
}
