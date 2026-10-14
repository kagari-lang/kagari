//! Scoped host-location access, independent of Rust layout and payload ownership.
use crate::ty::Ty;
use serde::{Deserialize, Serialize};

/// Owned leaves and nested inline struct locations are the initial field subset.
/// Schema metadata never grants Rust projection; installation supplies that proof.
pub fn field_read_type(ty: &Ty, access: ReferenceAccess, mutable: bool) -> Option<Ty> {
    match ty {
        Ty::Struct(_) => Some(Ty::Reference {
            target: Box::new(ty.clone()),
            access: access.field_access(mutable),
        }),
        Ty::Builtin(_) | Ty::NativeObject(_) => Some(ty.clone()),
        _ => None,
    }
}

/// Capability of a scope-bound host view. Mutable views may acquire a short shared
/// loan; shared views cannot acquire an exclusive loan or transfer ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReferenceAccess {
    Shared,
    Mutable,
}

impl ReferenceAccess {
    pub fn field_access(self, mutable: bool) -> Self {
        if mutable { self } else { Self::Shared }
    }

    pub fn permits(self, required: Self) -> bool {
        self == Self::Mutable || required == Self::Shared
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Shared => "Ref",
            Self::Mutable => "RefMut",
        }
    }
}
