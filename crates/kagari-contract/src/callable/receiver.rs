//! Storage-independent admission of a checked scoped method receiver.
use kagari_types::{reference::ReferenceAccess, ty::Ty};

/// A receiver access fact requires a nominal target (or its checked binder).
/// Application validation separately checks that concrete native storage is Opaque.
pub fn access_valid(access: Option<ReferenceAccess>, params: &[Ty]) -> bool {
    access.is_none() || params.first().is_some_and(Ty::reference_target_valid)
}

/// Admit a scoped location only for the same logical target and sufficient
/// declared capability. No GC object is reinterpreted as external Rust storage.
pub fn accepts_view(access: Option<ReferenceAccess>, target: &Ty, actual: &Ty) -> bool {
    let Ty::Reference {
        target: source,
        access: available,
    } = actual
    else {
        return false;
    };
    access.is_some_and(|required| available.permits(required) && source.as_ref() == target)
}
