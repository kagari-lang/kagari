//! Executable metadata edges are separate from immutable layout provenance.
pub(crate) mod application_key;
pub(crate) mod applications;
pub(crate) mod call_contracts;
mod environment_transport;
pub(crate) mod environments;
pub(crate) mod groups;
pub(crate) mod interfaces;
pub(crate) mod links;
pub(crate) mod operation;
mod publication;
pub(crate) mod scan_records;
pub(crate) mod scanning;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    closure::ClosureValueSnapshot,
    error::RuntimeError,
    execution_metadata::{
        applications::{ApplicationId, ApplicationStore},
        call_contracts::ScopedInterfaceCall,
        environments::{EnvironmentId, EnvironmentStore},
        groups::{OperationGroupId, OperationGroupStore, OperationId},
        interfaces::{InterfaceSnapshotId, InterfaceStore},
    },
    frame::types::{EnvironmentRecord, TypeEnvironment, operations::OperationBindings},
    gc::interfaces::{InterfaceDescription, InterfaceResultBinding, InterfaceValueSnapshot},
    module::ExecutableModule,
    native::{application::NativeApplication, stored_selection::StoredSelection},
};
use std::collections::HashSet;
#[cfg(test)]
use std::sync::Arc;

impl Runtime {
    pub(crate) fn validate_metadata(&self, edge: MetadataEdge<'_>) -> Result<(), RuntimeError> {
        self.inspect_metadata(edge, |_| {})
    }

    /// The application graph is immutable after construction. Keep a flat set of
    /// checked program dependencies for publication and reversible cache retention.
    pub(crate) fn metadata_dependencies(
        &self,
        edge: MetadataEdge<'_>,
    ) -> Result<Vec<ExecutableModule>, RuntimeError> {
        let mut dependencies = Vec::new();
        let mut seen = HashSet::new();
        self.inspect_metadata(edge, |owner| {
            if seen.insert(owner.program_identity()) {
                dependencies.push(owner.program_root());
            }
        })?;
        Ok(dependencies)
    }

    fn inspect_metadata(
        &self,
        edge: MetadataEdge<'_>,
        mut visit: impl FnMut(&ExecutableModule),
    ) -> Result<(), RuntimeError> {
        if let MetadataEdge::Program(owner) = edge {
            // Executable admission already checks availability and condemnation
            // on the same exact owner. Do not resolve it again as a graph view.
            self.validate_executable_module(owner)?;
            visit(owner);
            return Ok(());
        }
        self.gc.check_metadata_access(self.modules(), edge)?;
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::MetadataValidation);
        let mut error = None;
        let groups =
            self.gc.operation_groups.try_borrow().map_err(|_| {
                RuntimeError::module_validation("operation group store is borrowed")
            })?;
        let applications =
            self.gc.applications.try_borrow().map_err(|_| {
                RuntimeError::module_validation("method application store is borrowed")
            })?;
        let interfaces = self
            .gc
            .interfaces
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("interface store is borrowed"))?;
        let environments = self
            .gc
            .environments
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("environment store is borrowed"))?;
        MetadataTrace::new(&environments, &groups, &applications, &interfaces)
            .programs(edge, |owner| {
                self.validate_executable_module(owner)
                    .map(|()| visit(owner))
                    .map_err(|failure| error = Some(failure))
                    .ok()
            })
            .ok_or_else(|| {
                error.unwrap_or_else(|| {
                    RuntimeError::module_validation("invalid executable metadata")
                })
            })
    }
}

/// Stored in the central root table only for active execution or retained calls.
/// The same descriptors inside a heap object are ordinary graph edges.
#[derive(Debug, Clone)]
pub(crate) enum MetadataRoot {
    #[cfg(test)]
    NativeApplication(Arc<NativeApplication>),
    Program(ExecutableModule),
    Environment(EnvironmentId),
    Operation(OperationId),
    Interface(InterfaceSnapshotId),
    Application(ApplicationId),
}

impl MetadataRoot {
    pub(crate) fn edge(&self) -> MetadataEdge<'_> {
        match self {
            #[cfg(test)]
            Self::NativeApplication(value) => MetadataEdge::NativeApplication(value),
            Self::Program(value) => MetadataEdge::Program(value),
            Self::Environment(value) => MetadataEdge::Environment(*value),
            Self::Operation(value) => MetadataEdge::Operation(*value),
            Self::Interface(value) => MetadataEdge::Interface(*value),
            Self::Application(value) => MetadataEdge::Application(*value),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum MetadataEdge<'a> {
    NativeApplication(&'a NativeApplication),
    Program(&'a ExecutableModule),
    Environment(EnvironmentId),
    EnvironmentView(&'a EnvironmentRecord),
    InterfaceCall(&'a ScopedInterfaceCall),
    Operation(OperationId),
    Group(OperationGroupId),
    Interface(InterfaceSnapshotId),
    InterfaceView(&'a InterfaceValueSnapshot),
    InterfaceDescription(&'a InterfaceDescription),
    Application(ApplicationId),
    Binding(&'a InterfaceResultBinding),
    Closure(&'a ClosureValueSnapshot),
    Selections(&'a [StoredSelection]),
    Operations(&'a OperationBindings),
}

impl MetadataEdge<'_> {
    fn identity(self) -> Option<(u8, usize)> {
        Some(match self {
            Self::NativeApplication(value) => (8, value as *const _ as usize),
            Self::EnvironmentView(value) => (9, value as *const _ as usize),
            Self::InterfaceCall(value) => (10, value as *const _ as usize),
            Self::Program(value) => (0, value as *const _ as usize),
            Self::Environment(_)
            | Self::Operation(_)
            | Self::Group(_)
            | Self::Application(_)
            | Self::Interface(_) => {
                return None;
            }
            Self::InterfaceView(value) => (4, value as *const _ as usize),
            Self::InterfaceDescription(value) => (11, value as *const _ as usize),
            Self::Binding(value) => (6, value as *const _ as usize),
            Self::Closure(value) => (7, value as *const _ as usize),
            Self::Selections(_) | Self::Operations(_) => return None,
        })
    }
}

/// Addresses deduplicate borrowed immutable views during this traversal only;
/// program edges are subsequently checked against the owning runtime's store.
pub(crate) struct MetadataTrace<'a> {
    environments: &'a EnvironmentStore,
    live_environments: HashSet<EnvironmentId>,
    groups: &'a OperationGroupStore,
    interfaces: &'a InterfaceStore,
    live_interfaces: HashSet<InterfaceSnapshotId>,

    applications: &'a ApplicationStore,
    live_applications: HashSet<ApplicationId>,
    live_groups: HashSet<OperationGroupId>,
    seen_operations: HashSet<OperationId>,
    seen: HashSet<(u8, usize)>,
    pending: Vec<MetadataEdge<'a>>,
}

impl<'a> MetadataTrace<'a> {
    pub(crate) fn new(
        environments: &'a EnvironmentStore,
        groups: &'a OperationGroupStore,
        applications: &'a ApplicationStore,
        interfaces: &'a InterfaceStore,
    ) -> Self {
        Self {
            environments,
            live_environments: HashSet::new(),
            groups,
            interfaces,
            live_interfaces: HashSet::new(),

            applications,
            live_applications: HashSet::new(),
            live_groups: HashSet::new(),
            seen_operations: HashSet::new(),
            seen: HashSet::new(),
            pending: Vec::new(),
        }
    }

    pub(crate) fn programs(
        &mut self,
        edge: MetadataEdge<'a>,
        mut visit: impl FnMut(&'a ExecutableModule) -> Option<()>,
    ) -> Option<()> {
        self.pending.push(edge);
        while let Some(edge) = self.pending.pop() {
            if let Some(identity) = edge.identity()
                && !self.seen.insert(identity)
            {
                continue;
            }
            match edge {
                MetadataEdge::Program(owner) => visit(owner)?,
                MetadataEdge::Environment(id) => {
                    if self.live_environments.insert(id) {
                        self.environments.get(id)?.trace_metadata(&mut self.pending);
                    }
                }
                MetadataEdge::EnvironmentView(record) => record.trace_metadata(&mut self.pending),
                MetadataEdge::InterfaceCall(call) => call.trace(&mut self.pending),
                MetadataEdge::Group(id) => {
                    if self.live_groups.insert(id) {
                        self.groups.get(id)?.trace_metadata(&mut self.pending);
                    }
                }
                MetadataEdge::Operation(id) => {
                    if !self.seen_operations.insert(id) {
                        continue;
                    }
                    self.groups.operation(id)?;
                    self.pending.push(MetadataEdge::Group(id.group));
                }
                MetadataEdge::Interface(id) => {
                    if !self.live_interfaces.insert(id) {
                        continue;
                    }
                    self.pending
                        .push(MetadataEdge::InterfaceView(self.interfaces.get(id)?));
                }
                MetadataEdge::InterfaceView(snapshot) => {
                    self.pending
                        .push(MetadataEdge::InterfaceDescription(&snapshot.description));
                    for parent in &snapshot.parent_views {
                        if let Some(prepared) = parent.get() {
                            self.pending.push(MetadataEdge::Interface(*prepared));
                        }
                    }
                }
                MetadataEdge::InterfaceDescription(snapshot) => {
                    self.pending.extend(
                        (0..snapshot.scan_len()).filter_map(|field| snapshot.scan_field(field)),
                    );
                }
                MetadataEdge::Application(id) => {
                    if !self.live_applications.insert(id) {
                        continue;
                    }
                    self.applications.record(id)?.trace(&mut self.pending);
                }
                MetadataEdge::Binding(binding) => {
                    self.pending
                        .extend((0..2).filter_map(|field| binding.scan_field(field)));
                }
                MetadataEdge::Operations(operations) => {
                    operations.trace_metadata(&mut self.pending)
                }
                MetadataEdge::NativeApplication(application) => {
                    application.trace(&mut self.pending)
                }
                MetadataEdge::Selections(selections) => {
                    for selection in selections {
                        selection.trace(&mut self.pending);
                    }
                }
                MetadataEdge::Closure(closure) => {
                    // Ordinary routes retain captures and lexical evidence, not
                    // their original executable. Admission creates an exact edge.
                    if closure.call_route().is_none() {
                        self.pending
                            .push(MetadataEdge::Program(&closure.implementation));
                    }
                    self.environment(closure.environment.as_ref());
                }
            }
        }
        Some(())
    }

    fn environment(&mut self, environment: Option<&'a TypeEnvironment>) {
        if let Some(environment) = environment {
            self.pending.push(MetadataEdge::Environment(environment.id));
        }
    }
}
