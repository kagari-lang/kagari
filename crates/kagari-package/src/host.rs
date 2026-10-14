use crate::{
    error::PackageError,
    identity::{PackageFamily, PackageSelection, SourceRevision, source_namespace},
    manifest::{DependencySource, Manifest, PackageManifest},
    modules::VirtualSources,
};
use blake3::Hasher;
use kagari_common::cancellation::CancellationToken;

/// An immutable package revision explicitly supplied by a host.
///
/// Host keys are resolved only against the caller's catalog. This object contains
/// source, not native registrations or executable code. Its content fingerprint
/// includes the manifest and all supplied files, even unused ones.
#[derive(Debug, Clone)]
pub struct HostPackage {
    pub(crate) selection: PackageSelection,
    pub(crate) manifest: PackageManifest,
    pub(crate) sources: VirtualSources,
    pub(crate) fingerprint: String,
}

impl HostPackage {
    /// Captures one exact revision, validating metadata against its portable family.
    ///
    /// Host package dependencies must use other explicit host keys. Hosts that need
    /// a virtual path/workspace tree use `WorkspaceInputs` over `VirtualSources`.
    pub fn new(
        family: PackageFamily,
        revision: &str,
        manifest_text: &str,
        sources: VirtualSources,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        if PackageFamily::new(family.namespace(), family.directory(), family.name())? != family {
            return Err(PackageError::input(
                family.name(),
                "host family must use a normalized package locator",
            ));
        }
        if revision.is_empty() || revision.len() > 256 || revision.chars().any(char::is_control) {
            return Err(PackageError::input(
                family.name(),
                "host revision must be a nonempty bounded identifier",
            ));
        }
        let raw = Manifest::parse(family.name(), manifest_text, cancel)?;
        if raw.workspace.is_some()
            || raw
                .package
                .as_ref()
                .is_some_and(|package| package.workspace.is_some())
        {
            return Err(PackageError::input(
                family.name(),
                "exact host packages cannot declare workspace ownership",
            ));
        }
        let package = raw
            .package
            .as_ref()
            .ok_or_else(|| PackageError::input(family.name(), "host input requires [package]"))?;
        if package.name != family.name() {
            return Err(PackageError::input(
                family.name(),
                "host family and manifest package names differ",
            ));
        }
        source_namespace(package.namespace.as_deref(), Some(family.namespace()))?;
        let manifest = raw
            .normalize(family.name(), "", None, cancel)?
            .expect("package checked");
        if manifest
            .dependencies
            .values()
            .any(|dependency| matches!(dependency.source, DependencySource::Path(_)))
        {
            return Err(PackageError::input(
                family.name(),
                "exact host packages require host-key dependencies",
            ));
        }
        let mut hash = Hasher::new_derive_key("kagari immutable host input v1");
        hash_field(&mut hash, manifest_text.as_bytes());
        for (name, text) in &sources.files {
            cancel.check()?;
            hash_field(&mut hash, name.as_bytes());
            hash.update(&(text.len() as u64).to_le_bytes());
            for chunk in text.as_bytes().chunks(64 * 1024) {
                cancel.check()?;
                hash.update(chunk);
            }
        }
        cancel.check()?;
        Ok(Self {
            selection: PackageSelection {
                family,
                version: manifest.version.clone(),
                revision: SourceRevision::Host(revision.into()),
            },
            manifest,
            sources,
            fingerprint: hash.finalize().to_hex().to_string(),
        })
    }

    pub fn selection(&self) -> &PackageSelection {
        &self.selection
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

pub(crate) fn hash_field(hash: &mut Hasher, field: &[u8]) {
    hash.update(&(field.len() as u64).to_le_bytes());
    hash.update(field);
}
