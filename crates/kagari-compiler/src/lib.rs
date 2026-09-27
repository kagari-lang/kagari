//! Compiler orchestration and lowering from checked semantics to executable contracts.
pub mod bytecode;
#[cfg(feature = "source")]
pub mod source;
#[cfg(feature = "source")]
pub use source::lower::{MirLoweringError, MirLoweringOptions, lower_to_mir};
#[cfg(all(test, feature = "source"))]
mod tests;
