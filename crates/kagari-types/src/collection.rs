//! Access belongs to a typed reference, not to the shared collection object.
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
