//! Compiler orchestration and lowering from checked semantics to executable contracts.
pub mod bytecode;
pub mod native_input;
#[cfg(feature = "source")]
pub mod source;
#[cfg(all(test, feature = "source"))]
mod tests;
