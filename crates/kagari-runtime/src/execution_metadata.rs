//! Executable metadata edges are separate from immutable layout provenance.
pub(crate) mod application_key;
pub(crate) mod applications;
pub(crate) mod call_contracts;
pub(crate) mod environments;
pub(crate) mod groups;
pub(crate) mod interfaces;
pub(crate) mod links;
pub(crate) mod operation;
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
    gc::interfaces::{InterfaceResultBinding, InterfaceValueSnapshot},
    module::LoadedModule,
    native::{application::NativeApplication, stored_selection::StoredSelection},
    value::Value,
};
use std::{collections::HashSet, sync::Arc};

impl Runtime {
    pub(crate) fn validate_metadata(&self, edge: MetadataEdge<'_>) -> Result<(), RuntimeError> {
        self.inspect_metadata(edge, |_| {})
    }

    /// The application graph is immutable after construction. Keep a flat set of
    /// checked program dependencies for publication and reversible cache retention.
    pub(crate) fn metadata_dependencies(
        &self,
        edge: MetadataEdge<'_>,
    ) -> Result<Vec<LoadedModule>, RuntimeError> {
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
        mut visit: impl FnMut(&LoadedModule),
    ) -> Result<(), RuntimeError> {
        if let MetadataEdge::Program(owner) = edge {
            self.validate_loaded_module(owner)?;
            visit(owner);
            return Ok(());
        }
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
                self.validate_loaded_module(owner)
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
    NativeApplication(Arc<NativeApplication>),
    Program(LoadedModule),
    Environment(EnvironmentId),
    Operation(OperationId),
    Interface(InterfaceSnapshotId),
    Application(ApplicationId),
}

impl MetadataRoot {
    pub(crate) fn edge(&self) -> MetadataEdge<'_> {
        match self {
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
    Program(&'a LoadedModule),
    Environment(EnvironmentId),
    EnvironmentView(&'a EnvironmentRecord),
    InterfaceCall(&'a ScopedInterfaceCall),
    Operation(OperationId),
    Group(OperationGroupId),
    Interface(InterfaceSnapshotId),
    InterfaceView(&'a InterfaceValueSnapshot),
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
            Self::Binding(value) => (6, value as *const _ as usize),
            Self::Closure(value) => (7, value as *const _ as usize),
            Self::Selections(_) | Self::Operations(_) => return None,
        })
    }
}

pub(crate) struct MetadataLive {
    pub(crate) groups: HashSet<OperationGroupId>,
    pub(crate) environments: HashSet<EnvironmentId>,
    pub(crate) applications: HashSet<ApplicationId>,
    pub(crate) interfaces: HashSet<InterfaceSnapshotId>,
}

/// Addresses deduplicate borrowed immutable views during this traversal only;
/// program edges are subsequently checked against the owning runtime's store.
pub(crate) struct MetadataTrace<'a> {
    environments: &'a EnvironmentStore,
    live_environments: HashSet<EnvironmentId>,
    groups: &'a OperationGroupStore,
    interfaces: &'a InterfaceStore,
    live_interfaces: HashSet<InterfaceSnapshotId>,
    values: Vec<&'a Value>,
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
            values: Vec::new(),
            applications,
            live_applications: HashSet::new(),
            live_groups: HashSet::new(),
            seen_operations: HashSet::new(),
            seen: HashSet::new(),
            pending: Vec::new(),
        }
    }

    pub(crate) fn into_live_records(self) -> MetadataLive {
        MetadataLive {
            environments: self.live_environments,
            groups: self.live_groups,
            applications: self.live_applications,
            interfaces: self.live_interfaces,
        }
    }

    pub(crate) fn programs(
        &mut self,
        edge: MetadataEdge<'a>,
        mut visit: impl FnMut(&'a LoadedModule) -> Option<()>,
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
                    let operation = self.groups.operation(id)?;
                    self.pending.push(MetadataEdge::Group(id.group));
                    self.pending.push(MetadataEdge::Program(&operation.owner));
                    if let Some(generic) = &operation.generic {
                        self.pending
                            .push(MetadataEdge::Binding(&generic.receiver_table));
                        self.environment(generic.receiver_environment.as_ref());
                    }
                }
                MetadataEdge::Interface(id) => {
                    if !self.live_interfaces.insert(id) {
                        continue;
                    }
                    self.pending
                        .push(MetadataEdge::InterfaceView(self.interfaces.get(id)?));
                }
                MetadataEdge::InterfaceView(snapshot) => {
                    self.values.push(&snapshot.data);
                    self.pending
                        .push(MetadataEdge::Program(&snapshot.implementation));
                    self.pending
                        .push(MetadataEdge::Binding(&snapshot.receiver_table));
                    self.environment(snapshot.environment.as_ref());
                    if let Some(group) = snapshot.receiver_operations.get() {
                        self.pending.push(MetadataEdge::Group(*group));
                    }
                    for parent in &snapshot.parents {
                        self.pending.push(MetadataEdge::Binding(&parent.binding));
                        if let Some(prepared) = parent.prepared.get() {
                            self.pending.push(MetadataEdge::Interface(*prepared));
                        }
                    }
                    for method in snapshot.methods.iter().flatten() {
                        if let Some(Some(group)) = method.receiver_operations.get() {
                            self.pending.push(MetadataEdge::Group(*group));
                        }
                        if let Some(adapter) = &method.result_adapter {
                            self.pending.push(MetadataEdge::Binding(adapter));
                        }
                    }
                }
                MetadataEdge::Application(id) => {
                    if !self.live_applications.insert(id) {
                        continue;
                    }
                    let application = self.applications.get(id)?;
                    self.environment(application.environment.as_ref());
                    if let Some(adapter) = &application.result_adapter {
                        self.pending.push(MetadataEdge::Binding(adapter));
                    }
                }
                MetadataEdge::Binding(binding) => {
                    self.pending.push(MetadataEdge::Program(&binding.owner));
                    self.environment(binding.environment.as_ref());
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
                    self.pending
                        .push(MetadataEdge::Program(&closure.implementation));
                    self.environment(closure.environment.as_ref());
                }
            }
        }
        Some(())
    }

    pub(crate) fn append_values(&mut self, values: &mut Vec<&'a Value>) {
        values.append(&mut self.values);
    }

    fn environment(&mut self, environment: Option<&'a TypeEnvironment>) {
        if let Some(environment) = environment {
            self.pending.push(MetadataEdge::Environment(environment.id));
        }
    }
}
