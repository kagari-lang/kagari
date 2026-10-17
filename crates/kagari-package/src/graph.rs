//! Whole-workspace acquisition and exact dependency graphs.

mod resolve;

use crate::{
    error::PackageError,
    host::HostPackage,
    identity::{PackageFamily, PackageSelection},
    input::InputTree,
    lock::{Lockfile, ResolutionOptions},
    manifest::PackageManifest,
    workspace::{WorkspaceInputs, WorkspaceLimits},
};
use kagari_common::cancellation::CancellationToken;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Acquisition limits for a complete workspace dependency graph.
#[derive(Debug, Clone, Copy)]
pub struct GraphLimits {
    pub max_packages: usize,
    pub max_edges: usize,
    pub workspace: WorkspaceLimits,
}

impl Default for GraphLimits {
    fn default() -> Self {
        Self {
            max_packages: 4096,
            max_edges: 16384,
            workspace: WorkspaceLimits::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PackageSource {
    Local { directory: String },
    Host(Arc<HostPackage>),
}

/// A resolved package whose alias edges name exact selections.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub(crate) selection: PackageSelection,
    pub(crate) manifest: PackageManifest,
    pub(crate) dependencies: BTreeMap<String, PackageSelection>,
    pub(crate) source: PackageSource,
}

impl ResolvedPackage {
    pub fn selection(&self) -> &PackageSelection {
        &self.selection
    }

    pub fn dependencies(&self) -> &BTreeMap<String, PackageSelection> {
        &self.dependencies
    }

    pub fn manifest(&self) -> &PackageManifest {
        &self.manifest
    }
}

/// Immutable exact selections for all workspace members and their dependencies.
#[derive(Debug, Clone)]
pub struct ResolvedGraph {
    pub(crate) packages: BTreeMap<PackageSelection, ResolvedPackage>,
    /// Portable member locators relative to the workspace root.
    pub(crate) members: BTreeMap<String, PackageSelection>,
}

impl ResolvedGraph {
    /// Resolves every member without parsing any script body.
    ///
    /// Existing valid host selections are preserved. An unavailable still-valid
    /// locked revision is an error, not permission to silently upgrade it. Local
    /// paths are mutable and their current manifests determine exact versions/edges.
    pub fn resolve(
        input: &impl InputTree,
        workspace: &WorkspaceInputs,
        hosts: &BTreeMap<String, Arc<HostPackage>>,
        previous: Option<&Lockfile>,
        options: ResolutionOptions,
        limits: GraphLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        resolve::resolve(input, workspace, hosts, previous, options, limits, cancel)
    }

    pub fn packages(&self) -> &BTreeMap<PackageSelection, ResolvedPackage> {
        &self.packages
    }

    pub fn members(&self) -> &BTreeMap<String, PackageSelection> {
        &self.members
    }

    /// Extracts a dependency-complete Program closure, enforcing one selection per family.
    pub fn closure(
        &self,
        root: &PackageSelection,
        cancel: &CancellationToken,
    ) -> Result<BTreeSet<PackageSelection>, PackageError> {
        let mut seen = BTreeSet::new();
        let mut families: BTreeMap<&PackageFamily, (PackageSelection, Vec<String>)> =
            BTreeMap::new();
        let mut pending = vec![(root.clone(), vec![root.family.name().to_owned()])];
        while let Some((selection, chain)) = pending.pop() {
            cancel.check()?;
            if !seen.insert(selection.clone()) {
                continue;
            }
            let package = self.packages.get(&selection).ok_or_else(|| {
                PackageError::input(selection.family.name(), "missing resolved package")
            })?;
            if let Some((previous, previous_chain)) = families.insert(
                &package.selection.family,
                (selection.clone(), chain.clone()),
            ) && previous != selection
            {
                return Err(PackageError::input(
                    root.family.name(),
                    format!(
                        "conflicting selections of {}: {} versus {}",
                        selection.family.name(),
                        previous_chain.join(" -> "),
                        chain.join(" -> ")
                    ),
                ));
            }
            for (alias, dependency) in package.dependencies.iter().rev() {
                let mut next = chain.clone();
                next.push(format!(
                    "{alias} ({} {:?})",
                    dependency.version, dependency.revision
                ));
                pending.push((dependency.clone(), next));
            }
        }
        cancel.check()?;
        Ok(seen)
    }

    pub(crate) fn validate_cycles(&self, cancel: &CancellationToken) -> Result<(), PackageError> {
        validate_edges(
            self.packages
                .iter()
                .map(|(selection, package)| (selection, package.dependencies.values().collect()))
                .collect(),
            cancel,
        )
    }
}

pub(crate) fn validate_edges(
    edges: BTreeMap<&PackageSelection, Vec<&PackageSelection>>,
    cancel: &CancellationToken,
) -> Result<(), PackageError> {
    let mut completed = BTreeSet::new();
    for &root in edges.keys() {
        let mut active = BTreeSet::new();
        let mut trail: Vec<&PackageSelection> = Vec::new();
        let mut pending = vec![(root, false)];
        while let Some((selection, exit)) = pending.pop() {
            cancel.check()?;
            if exit {
                trail.pop();
                active.remove(selection);
                completed.insert(selection);
                continue;
            }
            if completed.contains(selection) {
                continue;
            }
            if !active.insert(selection) {
                return Err(PackageError::input(
                    selection.family.name(),
                    format!(
                        "package dependency cycle: {}",
                        trail
                            .iter()
                            .map(|entry| entry.family.name())
                            .chain([selection.family.name()])
                            .collect::<Vec<_>>()
                            .join(" -> ")
                    ),
                ));
            }
            trail.push(selection);
            pending.push((selection, true));
            let dependencies = edges.get(selection).ok_or_else(|| {
                PackageError::input(selection.family.name(), "missing graph edge target")
            })?;
            for &dependency in dependencies.iter().rev() {
                pending.push((dependency, false));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
