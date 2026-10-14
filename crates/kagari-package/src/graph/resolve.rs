use crate::{
    error::PackageError,
    graph::{GraphLimits, PackageSource, ResolvedGraph, ResolvedPackage},
    host::HostPackage,
    identity::{PackageSelection, SourceRevision},
    input::InputTree,
    lock::{Lockfile, ResolutionOptions},
    manifest::{DependencyRequirement, DependencySource},
    path,
    workspace::{WorkspaceInputs, WorkspacePackage},
};
use kagari_common::cancellation::CancellationToken;
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn resolve(
    input: &impl InputTree,
    workspace: &WorkspaceInputs,
    hosts: &BTreeMap<String, Arc<HostPackage>>,
    previous: Option<&Lockfile>,
    options: ResolutionOptions,
    limits: GraphLimits,
    cancel: &CancellationToken,
) -> Result<ResolvedGraph, PackageError> {
    cancel.check()?;
    if options.locked && previous.is_none() {
        return Err(PackageError::input(
            "Kagari.lock",
            "--locked requires an existing lock",
        ));
    }
    let mut host_selections = BTreeMap::new();
    for package in hosts.values() {
        cancel.check()?;
        if let Some(existing) = host_selections.insert(package.selection.clone(), package.clone())
            && existing.fingerprint != package.fingerprint
        {
            return Err(PackageError::input(
                package.selection.family.name(),
                "host revision supplied with different content",
            ));
        }
        if let Some(locked) = previous.and_then(|lock| lock.package(&package.selection))
            && locked.host_content.as_deref() != Some(&package.fingerprint)
        {
            return Err(PackageError::input(
                package.selection.family.name(),
                "host revision content differs from lock",
            ));
        }
    }
    let mut resolver = Resolver {
        input,
        hosts,
        host_selections,
        previous,
        limits,
        cancel,
        locals: workspace.members.clone(),
    };
    let mut pending = BTreeMap::new();
    let mut members = BTreeMap::new();
    for (directory, package) in &workspace.members {
        let resolved = local(directory, package);
        members.insert(
            path::relative(&workspace.directory, directory),
            resolved.selection.clone(),
        );
        enqueue(&mut pending, resolved)?;
    }
    let mut packages = BTreeMap::new();
    let mut edge_count = 0usize;
    while let Some((selection, mut package)) = pending.pop_first() {
        cancel.check()?;
        if packages.len() >= limits.max_packages.min(4096) {
            return Err(PackageError::input(
                selection.family.name(),
                "package graph exceeds limit",
            ));
        }
        for (alias, requirement) in &package.manifest.dependencies {
            edge_count += 1;
            if edge_count > limits.max_edges.min(16384) {
                return Err(PackageError::input(
                    &requirement.origin,
                    "dependency graph exceeds edge limit",
                ));
            }
            let dependency = resolver.dependency(&selection, alias, requirement)?;
            package
                .dependencies
                .insert(alias.clone(), dependency.selection.clone());
            if let Some(existing) = packages.get(&dependency.selection) {
                compatible_input(existing, &dependency)?;
            } else {
                enqueue(&mut pending, dependency)?;
            }
        }
        // A cycle can have enqueued the currently processed package again.
        pending.remove(&selection);
        packages.insert(selection, package);
    }
    let graph = ResolvedGraph { packages, members };
    graph.validate_cycles(cancel)?;
    for root in graph.members.values() {
        graph.closure(root, cancel)?;
    }
    Lockfile::from_graph(&graph).check_policy(previous, options)?;
    cancel.check()?;
    Ok(graph)
}

struct Resolver<'a, T> {
    input: &'a T,
    hosts: &'a BTreeMap<String, Arc<HostPackage>>,
    host_selections: BTreeMap<PackageSelection, Arc<HostPackage>>,
    previous: Option<&'a Lockfile>,
    limits: GraphLimits,
    cancel: &'a CancellationToken,
    locals: BTreeMap<String, WorkspacePackage>,
}

impl<T: InputTree> Resolver<'_, T> {
    fn dependency(
        &mut self,
        importer: &PackageSelection,
        alias: &str,
        requirement: &DependencyRequirement,
    ) -> Result<ResolvedPackage, PackageError> {
        self.cancel.check()?;
        let package = match &requirement.source {
            DependencySource::Path(directory) => {
                if !self.locals.contains_key(directory) {
                    let workspace = WorkspaceInputs::load(
                        self.input,
                        directory,
                        None,
                        self.limits.workspace,
                        self.cancel,
                    )?;
                    self.locals.extend(workspace.members);
                }
                let package = self.locals.get(directory).ok_or_else(|| {
                    PackageError::input(&requirement.origin, "dependency is not an owned package")
                })?;
                local(directory, package)
            }
            DependencySource::Host(key) => {
                let preferred = self.hosts.get(key).ok_or_else(|| {
                    PackageError::input(
                        &requirement.origin,
                        format!("host input {key} is unavailable"),
                    )
                })?;
                let mut package = preferred.clone();
                if let Some(edge) = self.previous.and_then(|lock| lock.edge(importer, alias))
                    && edge.host_key.as_deref() == Some(key)
                    && edge.package.family == preferred.selection.family
                    && matches_requirement(requirement, &edge.package)
                {
                    package = self
                        .host_selections
                        .get(&edge.package)
                        .cloned()
                        .ok_or_else(|| {
                            PackageError::input(
                                &requirement.origin,
                                "locked host input is unavailable; supply its exact revision",
                            )
                        })?;
                }
                ResolvedPackage {
                    selection: package.selection.clone(),
                    manifest: package.manifest.clone(),
                    dependencies: BTreeMap::new(),
                    source: PackageSource::Host(package),
                }
            }
        };
        if !matches_requirement(requirement, &package.selection) {
            return Err(PackageError::input(
                &requirement.origin,
                format!(
                    "selected {} {} does not satisfy the declared package/version requirement",
                    package.selection.family.name(),
                    package.selection.version
                ),
            ));
        }
        Ok(package)
    }
}

fn local(directory: &str, package: &WorkspacePackage) -> ResolvedPackage {
    ResolvedPackage {
        selection: PackageSelection {
            family: package.family.clone(),
            version: package.manifest.version.clone(),
            revision: SourceRevision::Local,
        },
        manifest: package.manifest.clone(),
        dependencies: BTreeMap::new(),
        source: PackageSource::Local {
            directory: directory.into(),
        },
    }
}

fn matches_requirement(requirement: &DependencyRequirement, selection: &PackageSelection) -> bool {
    requirement.package == selection.family.name()
        && requirement
            .version
            .as_ref()
            .is_none_or(|version| version.matches(&selection.version))
}

fn enqueue(
    pending: &mut BTreeMap<PackageSelection, ResolvedPackage>,
    package: ResolvedPackage,
) -> Result<(), PackageError> {
    if let Some(existing) = pending.get(&package.selection) {
        compatible_input(existing, &package)?;
    } else {
        pending.insert(package.selection.clone(), package);
    }
    Ok(())
}

fn compatible_input(a: &ResolvedPackage, b: &ResolvedPackage) -> Result<(), PackageError> {
    let same = match (&a.source, &b.source) {
        (PackageSource::Local { directory: a }, PackageSource::Local { directory: b }) => a == b,
        (PackageSource::Host(a), PackageSource::Host(b)) => a.fingerprint == b.fingerprint,
        _ => false,
    };
    if !same {
        return Err(PackageError::input(
            a.selection.family.name(),
            "one selection identifies different acquired inputs",
        ));
    }
    Ok(())
}
