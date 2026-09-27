//! Compiler orchestration and lowering from checked semantics to executable contracts.
pub mod bytecode;
pub mod native_input;
pub mod native_links;
#[cfg(feature = "source")]
pub mod source;
#[cfg(feature = "source")]
pub use source::lower::{MirLoweringError, MirLoweringOptions, lower_to_mir};
#[cfg(all(test, feature = "source"))]
mod tests;
