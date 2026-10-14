//! Scoped host-location access, independent of Rust layout and payload ownership.
use serde::{Deserialize, Serialize};

/// Capability of a scope-bound host view. Mutable views may acquire a short shared
/// loan; shared views cannot acquire an exclusive loan or transfer ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReferenceAccess {
    Shared,
    Mutable,
}

impl ReferenceAccess {
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
