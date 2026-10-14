//! Workspace discovery and package selection, before dependency graph resolution.

mod discovery;
mod loader;

use crate::{
    error::PackageError, identity::PackageFamily, input::InputTree, manifest::PackageManifest,
};
use kagari_common::cancellation::CancellationToken;
use std::collections::{BTreeMap, BTreeSet};

/// Resource limits for configuration discovery, independent of source-body limits.
#[derive(Debug, Clone, Copy)]
pub struct WorkspaceLimits {
    pub max_manifests: usize,
    pub max_directories: usize,
    pub max_depth: usize,
}

impl Default for WorkspaceLimits {
    fn default() -> Self {
        Self {
            max_manifests: 4096,
            max_directories: 16384,
            max_depth: 64,
        }
    }
}

/// A normalized workspace member, with its portable logical source identity.
#[derive(Debug, Clone)]
pub struct WorkspacePackage {
    pub family: PackageFamily,
    pub manifest: PackageManifest,
    pub origin: String,
}

/// One owning workspace (or standalone package) and its normalized members.
///
/// Directories are relative to the provider's approved root. Discovery does not read
/// script bodies, and membership does not add dependency edges or import privileges.
#[derive(Debug, Clone)]
pub struct WorkspaceInputs {
    pub directory: String,
    pub namespace: String,
    pub members: BTreeMap<String, WorkspacePackage>,
    pub default_members: BTreeSet<String>,
    /// The package selected by the invoking directory, if invocation is below the root.
    pub invoking_member: Option<String>,
}

/// CLI/host selection affects compile roots, never the whole-workspace lock scope.
#[derive(Debug, Clone, Default)]
pub enum MemberSelection {
    #[default]
    Default,
    Workspace,
    Names(Vec<String>),
}

impl WorkspaceInputs {
    /// Discovers a workspace from the directory containing the invoking manifest.
    ///
    /// Parent search stops at the approved input root. Explicit `package.workspace`
    /// paths use the member directory as their base. All members are normalized
    /// before a result is returned, even when the caller will select only one member.
    pub fn load(
        input: &impl InputTree,
        directory: &str,
        namespace: Option<&str>,
        limits: WorkspaceLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        loader::load(input, directory, namespace, limits, cancel)
    }

    /// Returns deterministic selected roots, rejecting unknown or empty explicit selections.
    pub fn select(
        &self,
        selection: &MemberSelection,
    ) -> Result<Vec<(&str, &WorkspacePackage)>, PackageError> {
        let directories = match selection {
            MemberSelection::Default => self
                .invoking_member
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            MemberSelection::Workspace => self.members.keys().cloned().collect(),
            MemberSelection::Names(names) => {
                if names.is_empty() {
                    return Err(PackageError::input(
                        &self.directory,
                        "empty package selection",
                    ));
                }
                let mut selected = BTreeSet::new();
                for name in names {
                    let (directory, _) = self
                        .members
                        .iter()
                        .find(|(_, package)| &package.manifest.name == name)
                        .ok_or_else(|| {
                            PackageError::input(
                                &self.directory,
                                format!("unknown workspace package {name}"),
                            )
                        })?;
                    selected.insert(directory.clone());
                }
                selected
            }
        };
        let directories = if matches!(selection, MemberSelection::Default) && directories.is_empty()
        {
            &self.default_members
        } else {
            &directories
        };
        directories
            .iter()
            .map(|directory| {
                let (directory, package) =
                    self.members.get_key_value(directory).ok_or_else(|| {
                        PackageError::input(
                            &self.directory,
                            format!("selection names absent member {directory}"),
                        )
                    })?;
                Ok((directory.as_str(), package))
            })
            .collect()
    }
}
