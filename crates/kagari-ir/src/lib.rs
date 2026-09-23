mod decode_limits;
mod lower;

pub mod bytecode;
pub mod module;
pub mod program;

pub use kagari_hir::builtin;
pub use lower::{IrLoweringError, IrLoweringOptions, lower_to_ir};

#[cfg(test)]
mod tests;
