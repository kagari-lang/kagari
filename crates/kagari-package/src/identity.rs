use crate::{error::PackageError, path};
use blake3::Hasher;
use kagari_common::identity::PackageId;
use semver::Version;
use serde::{Deserialize, Serialize};

/// Portable family identity, independent of checkout location and source edits.
///
/// The namespace is declared at the acquisition root or supplied by the host.
/// `directory` is the package's relative directory within that namespace, so two
/// equally named packages at different positions remain distinct. Version and
/// source content belong to selection/snapshot identity, not declaration identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PackageFamily {
    namespace: String,
    directory: String,
    name: String,
}

impl PackageFamily {
    /// Validates and captures a portable logical source family.
    pub fn new(namespace: &str, directory: &str, name: &str) -> Result<Self, PackageError> {
        validate_namespace(namespace)?;
        if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
            return Err(PackageError::input(
                namespace,
                "invalid family package name",
            ));
        }
        let directory = path::join(
            "",
            if directory.is_empty() { "." } else { directory },
            namespace,
        )?;
        Ok(Self {
            namespace: namespace.into(),
            directory,
            name: name.into(),
        })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn directory(&self) -> &str {
        &self.directory
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Derives the common semantic identity using an unambiguous domain-separated hash.
    /// The reserved `source-package:` prefix separates source families from installed
    /// provider spellings; installation still checks identity collisions explicitly.
    pub fn package_id(&self) -> PackageId {
        let mut hasher = Hasher::new_derive_key("kagari source package family v1");
        for field in [&self.namespace, &self.directory, &self.name] {
            hasher.update(&(field.len() as u64).to_le_bytes());
            hasher.update(field.as_bytes());
        }
        PackageId(format!("source-package:{}", hasher.finalize().to_hex()))
    }
}

/// Acquisition revision, separate from the current source snapshot's content hash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SourceRevision {
    /// Local source is mutable; editing it does not by itself rewrite the lock.
    Local,
    /// An exact immutable revision promised by the supplying host.
    Host(String),
}

/// An exact graph selection. A Program may contain only one selection per family.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PackageSelection {
    pub family: PackageFamily,
    pub version: Version,
    pub revision: SourceRevision,
}

/// Selects a namespace, rejecting disagreement between host and manifest inputs.
pub fn source_namespace(
    declared: Option<&str>,
    supplied: Option<&str>,
) -> Result<String, PackageError> {
    if let (Some(declared), Some(supplied)) = (declared, supplied)
        && declared != supplied
    {
        return Err(PackageError::input(
            "package namespace",
            "host and manifest namespaces disagree",
        ));
    }
    let value = supplied.or(declared).ok_or_else(|| {
        PackageError::input(
            "package namespace",
            "declare a root namespace or supply one from the host",
        )
    })?;
    validate_namespace(value)?;
    Ok(value.into())
}

fn validate_namespace(value: &str) -> Result<(), PackageError> {
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err(PackageError::input(
            "package namespace",
            "namespace must be 1..256 bytes without whitespace or control characters",
        ));
    }
    Ok(())
}
