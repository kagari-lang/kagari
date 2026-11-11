//! The SDK exposes the canonical executable Runtime.
pub use kagari_runtime::{
    Runtime,
    loading::{LoadOptions, ReloadOptions},
};
#[cfg(all(test, feature = "source", feature = "native"))]
mod language_contract;
#[cfg(test)]
mod tests;
