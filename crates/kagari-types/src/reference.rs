//! Short Rust payload loan capabilities, independent of script value representation.
use serde::{Deserialize, Serialize};

/// Required or granted access for a short Rust payload loan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReferenceAccess {
    Shared,
    Mutable,
}
