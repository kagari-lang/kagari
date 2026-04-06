mod lower;

pub mod bytecode;
pub mod module;

pub use lower::{IrLoweringError, lower_to_ir};

#[cfg(test)]
mod tests;
