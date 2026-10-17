//! Portable, bounded whole-workspace lock records and explicit update policy.

mod filesystem;

use crate::{
    error::PackageError,
    graph::{PackageSource, ResolvedGraph, validate_edges},
    identity::{PackageFamily, PackageSelection, SourceRevision},
    manifest::DependencySource,
    path,
};
use kagari_common::cancellation::CancellationToken;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use toml::{from_str, to_string_pretty};

pub const MAX_LOCK_BYTES: usize = 16 * 1024 * 1024;

/// Locked prohibits graph changes; offline prohibits network acquisition.
/// All currently implemented providers are local. Frozen means both flags are true.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolutionOptions {
    pub locked: bool,
    pub offline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LockedEdge {
    pub package: PackageSelection,
    pub host_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LockedPackage {
    pub selection: PackageSelection,
    pub host_content: Option<String>,
    pub dependencies: BTreeMap<String, LockedEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    format: u32,
    pub(crate) packages: Vec<LockedPackage>,
    members: BTreeMap<String, PackageSelection>,
}

impl Lockfile {
    pub fn from_graph(graph: &ResolvedGraph) -> Self {
        Self {
            format: 1,
            members: graph.members.clone(),
            packages: graph
                .packages
                .values()
                .map(|package| LockedPackage {
                    selection: package.selection.clone(),
                    host_content: match &package.source {
                        PackageSource::Local { .. } => None,
                        PackageSource::Host(host) => Some(host.fingerprint.clone()),
                    },
                    dependencies: package
                        .dependencies
                        .iter()
                        .map(|(alias, selection)| {
                            let host_key = match &package.manifest.dependencies[alias].source {
                                DependencySource::Path(_) => None,
                                DependencySource::Host(key) => Some(key.clone()),
                            };
                            (
                                alias.clone(),
                                LockedEdge {
                                    package: selection.clone(),
                                    host_key,
                                },
                            )
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Decodes bounded TOML and rejects malformed, duplicated or dangling graph records.
    pub fn parse(
        origin: &str,
        text: &str,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        cancel.check()?;
        if text.len() > MAX_LOCK_BYTES {
            return Err(PackageError::input(origin, "lock exceeds byte limit"));
        }
        let mut lock = from_str::<Self>(text).map_err(|error| PackageError::Input {
            origin: origin.into(),
            range: error.span(),
            message: error.message().into(),
        })?;
        if lock.format != 1 || lock.packages.len() > 4096 || lock.members.len() > 4096 {
            return Err(PackageError::input(
                origin,
                "unsupported lock format or graph size",
            ));
        }
        let mut selections = BTreeSet::new();
        let mut local_families = BTreeSet::new();
        let mut edges = 0usize;
        for package in &lock.packages {
            cancel.check()?;
            let family = &package.selection.family;
            if PackageFamily::new(family.namespace(), family.directory(), family.name())? != *family
                || !selections.insert(&package.selection)
            {
                return Err(PackageError::input(
                    origin,
                    "invalid or duplicate locked selection",
                ));
            }
            edges = edges.saturating_add(package.dependencies.len());
            if edges > 16384 {
                return Err(PackageError::input(origin, "lock edge limit exceeded"));
            }
            match (&package.selection.revision, &package.host_content) {
                (SourceRevision::Local, None) if local_families.insert(family) => {}
                (SourceRevision::Host(revision), Some(hash))
                    if !revision.is_empty()
                        && revision.len() <= 256
                        && !revision.chars().any(char::is_control)
                        && hash.len() == 64
                        && hash
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) => {
                }
                _ => {
                    return Err(PackageError::input(
                        origin,
                        "invalid locked source revision or integrity hash",
                    ));
                }
            }
        }
        for (member, selection) in &lock.members {
            if path::locator(member, origin)? != *member
                || selection.revision != SourceRevision::Local
            {
                return Err(PackageError::input(
                    origin,
                    "invalid locked member locator or source",
                ));
            }
        }
        for package in &lock.packages {
            for edge in package.dependencies.values() {
                match (&edge.package.revision, &edge.host_key) {
                    (SourceRevision::Local, None) => {}
                    (SourceRevision::Host(_), Some(key)) if !key.is_empty() => {}
                    _ => {
                        return Err(PackageError::input(
                            origin,
                            "locked dependency source disagrees with its target",
                        ));
                    }
                }
            }
        }
        for selection in lock.members.values().chain(
            lock.packages
                .iter()
                .flat_map(|package| package.dependencies.values().map(|edge| &edge.package)),
        ) {
            cancel.check()?;
            if !selections.contains(selection) {
                return Err(PackageError::input(
                    origin,
                    "dangling locked graph reference",
                ));
            }
        }
        validate_edges(
            lock.packages
                .iter()
                .map(|package| {
                    (
                        &package.selection,
                        package
                            .dependencies
                            .values()
                            .map(|edge| &edge.package)
                            .collect(),
                    )
                })
                .collect(),
            cancel,
        )?;
        lock.packages.sort_by(|a, b| a.selection.cmp(&b.selection));
        cancel.check()?;
        Ok(lock)
    }

    pub fn encode(&self) -> Result<String, PackageError> {
        to_string_pretty(self)
            .map_err(|error| PackageError::input("Kagari.lock", error.to_string()))
    }

    pub(crate) fn edge(&self, importer: &PackageSelection, alias: &str) -> Option<&LockedEdge> {
        let exact = self
            .packages
            .iter()
            .find(|package| &package.selection == importer);
        let package = exact.or_else(|| {
            (importer.revision == SourceRevision::Local)
                .then(|| {
                    self.packages.iter().find(|package| {
                        package.selection.family == importer.family
                            && package.selection.revision == SourceRevision::Local
                    })
                })
                .flatten()
        })?;
        package.dependencies.get(alias)
    }

    pub(crate) fn package(&self, selection: &PackageSelection) -> Option<&LockedPackage> {
        self.packages
            .iter()
            .find(|package| &package.selection == selection)
    }

    pub(crate) fn check_policy(
        &self,
        previous: Option<&Self>,
        options: ResolutionOptions,
    ) -> Result<(), PackageError> {
        if options.locked && previous != Some(self) {
            return Err(PackageError::input(
                "Kagari.lock",
                "--locked requires an existing unchanged whole-workspace lock",
            ));
        }
        Ok(())
    }
}
