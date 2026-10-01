//! Installed standard-library sources and structural syntax preparation.
//!
//! This crate neither resolves types nor authorizes execution. Only the bundled
//! package can be prepared through this API; a user-controlled URI cannot turn
//! a source file into an installed engine declaration.
pub mod index;
pub mod manifest;
pub mod package;

#[cfg(test)]
mod tests;
