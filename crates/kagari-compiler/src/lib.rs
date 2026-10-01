//! Compiler orchestration and lowering from checked semantics to executable contracts.
pub mod bytecode;
pub mod native_input;
pub mod native_links;
#[cfg(feature = "source")]
pub mod source;
#[cfg(all(test, feature = "source"))]
mod tests;
