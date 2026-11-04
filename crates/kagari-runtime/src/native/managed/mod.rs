//! Pre-cutover Native access components for a separately owned managed heap.
//!
//! These components do not replace Runtime's production collector or accept its
//! Values. They establish the concrete scope/slot/provider boundary before that
//! coherent migration. Immediate scalars and typed native objects are admitted here;
//! this is not the full language Value representation or a second executor.

pub mod arguments;
pub mod call;
pub mod entry;
pub mod error;
pub mod payload;
pub mod slots;

pub mod value;

#[cfg(test)]
mod tests;
