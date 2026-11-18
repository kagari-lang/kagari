//! Admission of a declared Rust payload loan on a method receiver.
use kagari_types::{callable::receiver_type_valid, reference::ReferenceAccess, ty::Ty};

/// A receiver access fact requires a nominal target (or its checked binder).
/// Application validation checks the installed Rust mapping and storage source.
pub fn access_valid(access: Option<ReferenceAccess>, params: &[Ty]) -> bool {
    access.is_none() || params.first().is_some_and(receiver_type_valid)
}

/// Only runtime-owned native storage can lend a payload. Generic receivers
/// are checked again at application.
pub fn accepts_owned(access: Option<ReferenceAccess>, target: &Ty) -> bool {
    access.is_none() || matches!(target, Ty::NativeObject(_) | Ty::Parameter { .. })
}
