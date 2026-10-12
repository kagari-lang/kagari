//! Legacy concrete Map/Set host access; ordinary traits carry no access marker.
//! The descriptor belongs to the typed reference, not to shared object state.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CollectionAccess {
    ReadOnly,
    Mutable,
}

impl CollectionAccess {
    pub fn permits(self, required: Self) -> bool {
        self == required || required == Self::ReadOnly
    }
}
