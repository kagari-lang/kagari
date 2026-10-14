//! Project acquisition shared by hosts and command-line tooling.

use crate::{
    error::PackageError,
    graph::{GraphLimits, ResolvedGraph},
    host::HostPackage,
    input::InputTree,
    lock::{Lockfile, ResolutionOptions},
    modules::ModuleLimits,
    path,
    snapshot::ProgramInputs,
    workspace::{MemberSelection, WorkspaceInputs},
};
use kagari_common::cancellation::CancellationToken;
use std::{collections::BTreeMap, sync::Arc};

/// Acquisition and selection policy for one project request.
#[derive(Debug, Clone, Default)]
pub struct ProjectOptions {
    /// Must match a namespace declared at the owning manifest root, if present.
    pub namespace: Option<String>,
    pub selection: MemberSelection,
    /// Explicit immutable providers; dependency keys never initiate remote lookup.
    pub hosts: BTreeMap<String, Arc<HostPackage>>,
    pub resolution: ResolutionOptions,
    pub graph_limits: GraphLimits,
    /// Capture limits apply to each selected Program's complete closure.
    pub module_limits: ModuleLimits,
}

/// One resolved workspace and isolated immutable inputs for its selected roots.
///
/// The lock covers all members, including ones whose bodies were not captured.
/// Compilation and execution remain separate from acquisition. Retaining this
/// snapshot keeps the previous sources even after the provider changes.
#[derive(Debug, Clone)]
pub struct ProjectInputs {
    workspace: WorkspaceInputs,
    graph: ResolvedGraph,
    lock: Lockfile,
    programs: BTreeMap<String, ProgramInputs>,
}

impl ProjectInputs {
    /// Resolves a project and captures each selected root without writing files.
    ///
    /// `directory` names the invoking manifest directory relative to the approved
    /// input root. Discovery/inheritance, selection and locking use their existing
    /// owners. Filesystem and virtual inputs therefore follow exactly the same
    /// policy. The existing lock is read from the discovered workspace root.
    /// Publish `lockfile()` separately through `Lockfile::publish` when desired.
    ///
    /// Returns manifest/selection/lock/capture errors or cancellation. No partial
    /// project is returned. Mutable filesystem providers retain their documented
    /// host-controlled mutation assumption; this is not an OS snapshot operation.
    pub fn load(
        input: &impl InputTree,
        directory: &str,
        options: &ProjectOptions,
        cancel: &CancellationToken,
    ) -> Result<Self, PackageError> {
        let workspace = WorkspaceInputs::load(
            input,
            directory,
            options.namespace.as_deref(),
            options.graph_limits.workspace,
            cancel,
        )?;
        let selected = workspace.select(&options.selection)?;
        let previous = Lockfile::load(input, &workspace.directory, cancel)?;
        let graph = ResolvedGraph::resolve(
            input,
            &workspace,
            &options.hosts,
            previous.as_ref(),
            options.resolution,
            options.graph_limits,
            cancel,
        )?;
        let mut programs = BTreeMap::new();
        for (directory, _) in selected {
            let member = path::relative(&workspace.directory, directory);
            let root = &graph.members()[&member];
            programs.insert(
                member,
                graph.capture(input, root, options.module_limits, cancel)?,
            );
        }
        cancel.check()?;
        Ok(Self {
            lock: Lockfile::from_graph(&graph),
            workspace,
            graph,
            programs,
        })
    }

    pub fn workspace(&self) -> &WorkspaceInputs {
        &self.workspace
    }

    pub fn graph(&self) -> &ResolvedGraph {
        &self.graph
    }

    pub fn lockfile(&self) -> &Lockfile {
        &self.lock
    }

    /// Selected roots keyed by portable member directory relative to the workspace.
    /// Each value contains only that root's dependency closure, not sibling members.
    pub fn programs(&self) -> &BTreeMap<String, ProgramInputs> {
        &self.programs
    }
}
