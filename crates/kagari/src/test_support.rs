#[cfg(feature = "source")]
pub(crate) mod cancellation;
#[cfg(all(feature = "source", feature = "native"))]
pub(crate) mod execution;

// Share explicit raw proof preparation with the runtime contract fixtures.
#[cfg(feature = "source")]
#[path = "../../kagari-runtime/tests/support/bytecode.rs"]
pub(crate) mod bytecode;

#[cfg(feature = "source")]
use crate::engine::KagariEngine;

// Same immutable authoring input as the public SDK integration targets.
#[cfg(feature = "source")]
#[path = "../tests/support/standard.rs"]
pub(crate) mod standard;
