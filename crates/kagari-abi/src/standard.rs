//! Closed language representation and protocol bridges. Public library calls use providers.
pub mod surface;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuntimePrimitive {
    ValuePartialCmp,
    ValueCmp,
    ValueEq,
    ValueHash,
    ValueDebug,
    ValueDisplay,
    StringPartsJoin,
    Assert,
}
