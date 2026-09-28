//! Installed standard-library sources and structural syntax preparation.
//!
//! This crate neither resolves types nor authorizes execution. Only the bundled
//! package can be prepared through this API; a user-controlled URI cannot turn
//! a source file into an installed engine declaration.
mod index;
mod manifest;
mod package;

pub use index::{DeclarationSite, NativeMarker, NativeMarkerKind};
pub use manifest::{BundledSource, bundled_sources};
pub use package::{PackageError, ParsedStdlibFile, ParsedStdlibPackage};

#[cfg(test)]
mod tests;
