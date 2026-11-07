//! Owned recovering source inputs with explicit physical/semantic provenance.
//!
//! These inputs support editor analysis, never strict compilation admission. No
//! syntax arenas, databases, filesystem handles or protocol URIs cross this boundary.
use crate::{
    error::PackageError,
    graph::PackageSource,
    modules::{ModuleDiscovery, SourceText},
    project::ProjectDiscovery,
    snapshot::registration,
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_source::{
    identity::FileId,
    package::PackageEnvironment,
    source_database::{SourceDatabase, SourceSnapshot},
};
use std::collections::{BTreeMap, BTreeSet};

/// One available physical source registered at its exact logical module.
/// Inline children share this source and are discovered by the semantic owner.
#[derive(Debug, Clone)]
pub struct ToolingSource {
    pub module: ModuleIdentity,
    pub source: SourceText,
    /// Host keys belong to their package provider, not to the local input tree.
    pub host: bool,
}

/// Source registration provenance, independent of protocol URI presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOrigin {
    pub module: ModuleIdentity,
    /// Exact `SourceText.key`; do not reconstruct it from `display_name`.
    pub provider_key: String,
    pub display_name: String,
    pub host: bool,
}

/// A committed source revision and its matching physical-origin map.
#[derive(Debug, Clone)]
pub struct ToolingSnapshot {
    pub sources: SourceSnapshot,
    pub origins: BTreeMap<FileId, SourceOrigin>,
}

/// Immutable available units and resolved package scopes, including incomplete roots.
/// Unavailable modules have no fabricated source entry. Acquisition errors are
/// retained separately; syntax diagnostics are produced from the retained text by HIR.
#[derive(Debug, Clone)]
pub struct ToolingInputs {
    root: ModuleIdentity,
    sources: Vec<ToolingSource>,
    environment: PackageEnvironment,
    complete: bool,
    errors: Vec<PackageError>,
}

impl ToolingInputs {
    pub(crate) fn from_project(
        project: &ProjectDiscovery,
        member: &str,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        cancel.check()?;
        let capture = project
            .roots
            .get(member)
            .ok_or_else(|| PackageError::input(member, "member has no source capture"))?;
        let root = project
            .graph
            .members()
            .get(member)
            .ok_or_else(|| PackageError::input(member, "member has no resolved package"))?;
        let closure = project.graph.closure(root, cancel)?;
        let mut scopes = BTreeMap::new();
        let mut sources = Vec::new();
        let mut errors = capture.errors.clone();
        for selection in closure {
            cancel.check()?;
            let package = &project.graph.packages()[&selection];
            let id = selection.family.package_id();
            scopes.insert(
                id.clone(),
                registration::scope(&selection, package.dependencies()),
            );
            let discovery = capture.discoveries.get(&selection).ok_or_else(|| {
                PackageError::input(member, "resolved closure is missing its discovery result")
            })?;
            collect(
                &mut sources,
                discovery,
                &ModuleIdentity {
                    package: id,
                    path: Vec::new(),
                },
                matches!(package.source, PackageSource::Host(_)),
                cancel,
            )?;
            errors.extend_from_slice(discovery.errors());
        }
        validate_origins(&sources)?;
        let environment = PackageEnvironment::new(scopes)
            .map_err(|message| PackageError::input(member, message))?;
        cancel.check()?;
        Ok(Self {
            root: ModuleIdentity {
                package: root.family.package_id(),
                path: Vec::new(),
            },
            sources,
            environment,
            complete: capture.program.is_some(),
            errors,
        })
    }

    /// Retains a standalone declared module tree under the caller's source identity.
    /// `root` supplies the family and base module path; no implicit package imports
    /// or filesystem reads are added. Incomplete discovery remains incomplete.
    pub fn from_modules(
        discovery: &ModuleDiscovery,
        root: ModuleIdentity,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        cancel.check()?;
        let mut sources = Vec::new();
        collect(&mut sources, discovery, &root, false, cancel)?;
        validate_origins(&sources)?;
        cancel.check()?;
        Ok(Self {
            root,
            sources,
            environment: PackageEnvironment::default(),
            complete: discovery.is_complete(),
            errors: discovery.errors().to_vec(),
        })
    }

    pub fn root(&self) -> &ModuleIdentity {
        &self.root
    }

    pub fn sources(&self) -> &[ToolingSource] {
        &self.sources
    }

    pub fn environment(&self) -> &PackageEnvironment {
        &self.environment
    }

    /// Complete acquisition is useful for coverage decisions, not compile authority.
    pub fn complete(&self) -> bool {
        self.complete
    }

    pub fn errors(&self) -> &[PackageError] {
        &self.errors
    }

    pub fn total_bytes(&self) -> usize {
        self.sources
            .iter()
            .map(|unit| unit.source.text().len())
            .sum()
    }

    /// Atomically publishes available text, package scopes and exact origin bindings.
    /// Unchanged modules retain file IDs/revisions; cancellation or invalid input
    /// preserves the previous database. The returned map names only this snapshot.
    pub fn source_snapshot(
        &self,
        database: &mut SourceDatabase,
        cancel: &CancellationToken,
    ) -> Result<ToolingSnapshot, PackageError> {
        cancel.check()?;
        let mut captured = Vec::with_capacity(self.sources.len());
        let mut origins_by_module = BTreeMap::new();
        for unit in &self.sources {
            cancel.check()?;
            captured.push(registration::source(unit.module.clone(), &unit.source));
            origins_by_module.insert(
                unit.module.clone(),
                SourceOrigin {
                    module: unit.module.clone(),
                    provider_key: unit.source.key.clone(),
                    display_name: unit.source.origin.clone(),
                    host: unit.host,
                },
            );
        }
        let sources = registration::commit(database, captured, self.environment.clone(), cancel)?;
        // All fallible construction and cancellation checks precede the source
        // transaction. The remaining projection consumes only the committed units.
        let origins = sources
            .files()
            .map(|file| {
                let origin = origins_by_module
                    .remove(file.module_identity())
                    .expect("committed module has a captured origin");
                (file.id(), origin)
            })
            .collect();
        assert!(
            origins_by_module.is_empty(),
            "source commit omitted a captured module"
        );
        Ok(ToolingSnapshot { sources, origins })
    }
}

fn collect(
    sources: &mut Vec<ToolingSource>,
    discovery: &ModuleDiscovery,
    root: &ModuleIdentity,
    host: bool,
    cancel: &CancellationToken,
) -> Result<(), PackageError> {
    for (path, location) in discovery.modules() {
        cancel.check()?;
        if location.inline_range.is_some() {
            continue;
        }
        // Failed registration can retain the intended module while acquisition
        // has no source (for example, an alias rejected before reading its text).
        let Some(source) = discovery.files().get(&location.file) else {
            continue;
        };
        let mut module = root.clone();
        module.path.extend_from_slice(path);
        if !module.within_path_limit() {
            return Err(PackageError::input(
                &source.origin,
                "module identity path exceeds limit",
            ));
        }
        sources.push(ToolingSource {
            module,
            source: source.clone(),
            host,
        });
    }
    Ok(())
}

fn validate_origins(sources: &[ToolingSource]) -> Result<(), PackageError> {
    let mut identities = BTreeSet::new();
    let mut physical = BTreeMap::new();
    for unit in sources {
        if !identities.insert(&unit.module) {
            return Err(PackageError::input(
                &unit.source.origin,
                "duplicate tooling module identity",
            ));
        }
        if !unit.host
            && let Some(previous) = physical.insert(&unit.source.key, &unit.module)
        {
            return Err(PackageError::input(
                &unit.source.origin,
                format!(
                    "physical source shared by modules {previous} and {}",
                    unit.module
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
