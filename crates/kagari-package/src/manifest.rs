//! Strict manifest parsing and dependency inheritance, independent of IO.

mod normalize;

#[cfg(test)]
mod tests;

use crate::error::PackageError;
use kagari_common::cancellation::CancellationToken;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::collections::BTreeMap;
use toml::from_str;

pub const MANIFEST_NAME: &str = "Kagari.toml";
pub const LOCK_NAME: &str = "Kagari.lock";
pub const DEFAULT_ROOT: &str = "src/lib.kgr";
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Parsed configuration. Use [`Manifest::normalize`] to apply dependency inheritance.
///
/// Unknown fields are errors, including unsupported Cargo features. A virtual
/// workspace has `workspace` but no `package` and cannot declare dependencies.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub package: Option<Package>,
    pub workspace: Option<Workspace>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}

/// One package's metadata; its root is a module, not an execution entrypoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub version: Version,
    #[serde(default = "default_root")]
    pub root: String,
    /// Explicit owning workspace directory, relative to this manifest.
    pub workspace: Option<String>,
    /// Portable source namespace for a standalone acquisition root.
    pub namespace: Option<String>,
}

/// Cargo-style package grouping and explicit dependency inheritance.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Workspace {
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub default_members: Option<Vec<String>>,
    pub namespace: Option<String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}

/// A dependency declaration. Initial acquisition supports paths and host inputs.
///
/// `host` names an explicitly supplied host package key; it never reads a global
/// registry or grants access to runtime registration. `version` constrains the
/// supplied package, using Cargo-compatible semantic version requirements.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub package: Option<String>,
    pub version: Option<VersionReq>,
    pub path: Option<String>,
    pub host: Option<String>,
    pub workspace: Option<bool>,
}

/// A normalized source request with an unambiguous path base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencySource {
    /// Portable directory relative to the approved input root.
    Path(String),
    /// An exact entry in the caller's immutable host input collection.
    Host(String),
}

/// A direct dependency after explicit workspace inheritance and validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyRequirement {
    pub package: String,
    pub version: Option<VersionReq>,
    pub source: DependencySource,
    /// Origin of the actual declaration, including the dependency alias.
    pub origin: String,
}

/// Validated package inputs ready for graph acquisition.
#[derive(Debug, Clone)]
pub struct PackageManifest {
    pub name: String,
    pub version: Version,
    /// Source path relative to the approved input root.
    pub root: String,
    pub dependencies: BTreeMap<String, DependencyRequirement>,
}

/// The owning workspace's declarations and their input-root-relative directory.
pub struct Inheritance<'a> {
    pub directory: &'a str,
    pub manifest_origin: &'a str,
    pub workspace: &'a Workspace,
}

impl Manifest {
    /// Parses bounded TOML, preserving parser byte ranges in diagnostics.
    ///
    /// Returns an error for unknown fields, invalid versions, missing package and
    /// workspace tables, or cancellation. Normalization checks dependency semantics.
    pub fn parse(
        origin: &str,
        text: &str,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        cancel.check()?;
        if text.len() > MAX_MANIFEST_BYTES {
            return Err(PackageError::input(origin, "manifest exceeds byte limit"));
        }
        let manifest = from_str::<Self>(text).map_err(|error| PackageError::Input {
            origin: origin.into(),
            range: error.span(),
            message: error.message().into(),
        })?;
        manifest.validate(origin)?;
        cancel.check()?;
        Ok(manifest)
    }
}

fn default_root() -> String {
    DEFAULT_ROOT.into()
}
