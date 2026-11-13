//! Preparation borrows actual graph sources instead of reconstructing them by ID.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{DependencyId, ExecutionRoots, Slots, ValueLocation},
    execution_metadata::managed::{
        ProtectedGraph, ProtectedMetadata, applications::ApplicationObject,
        environments::EnvironmentObject, groups::OperationGroupObject,
    },
    execution_metadata::{
        applications::AdmittedApplication, environments::AdmittedEnvironment,
        groups::AdmittedOperationGroup,
    },
    module::{ExecutableModule, ModuleStore, managed::source::RevisionSource},
    native::future::storage::ColdObject,
    resource::ResourceState,
    value::{
        carriers::RootedValue, closures::ClosureObject, interfaces::InterfaceObject,
        native::NativeObject,
    },
};
use kagari_gc::{access::Scope, error::GcError, root::Root};
use std::collections::TryReserveError;

/// Borrowed candidates for a closed metadata operation. A preparation may append
/// newly published dependency coordinates; this list owns no managed roots and
/// cannot extend its frame/Native/preparation source suffix's lifetime.
#[derive(Clone, Copy)]
pub(crate) enum MetadataSource<'origin> {
    Revision {
        owner: &'origin ExecutableModule,
        source: RevisionSource<'origin>,
    },
    Environment(DependencyId<EnvironmentObject>),
    AdmittedEnvironment(&'origin AdmittedEnvironment),
    Operations(DependencyId<OperationGroupObject>),
    AdmittedOperations(&'origin AdmittedOperationGroup),
    Application(DependencyId<ApplicationObject>),
    AdmittedApplication(&'origin AdmittedApplication),
    HostEnvironment(&'origin Root<EnvironmentObject>),
    HostApplication(&'origin Root<ApplicationObject>),
    Cold(&'origin Root<ColdObject>),
    ColdClosureEnvironment(&'origin Root<ColdObject>),
    RootedClosureEnvironment(&'origin RootedValue),
    /// Follow only the metadata edge of this actual protected Native object.
    NativeSelections(ValueLocation),
    ClosureEnvironment(ValueLocation),
    Interface(ValueLocation),
    RootedInterface(&'origin RootedValue),
}

impl MetadataSource<'_> {
    pub(crate) fn resolve<'gc>(
        &self,
        modules: &ModuleStore,
        slots: &mut Slots<'_, 'gc>,
    ) -> Result<ProtectedMetadata<'gc>, GcError> {
        Ok(match self {
            Self::RootedClosureEnvironment(root) => {
                let value = root.get(slots.scope())?;
                let closure = slots
                    .scope()
                    .downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                ProtectedMetadata::Environment(
                    ClosureObject::environment(slots.scope(), closure)
                        .ok_or(GcError::Representation)?,
                )
            }
            Self::Cold(root) => ProtectedMetadata::Cold(slots.scope().get(root)?),
            Self::ColdClosureEnvironment(root) => {
                let cold = slots.scope().get(root)?;
                let value = ColdObject::capture(slots.scope(), cold, 0)?;
                let closure = slots
                    .scope()
                    .downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                ProtectedMetadata::Environment(
                    ClosureObject::environment(slots.scope(), closure)
                        .ok_or(GcError::Representation)?,
                )
            }
            Self::RootedInterface(root) => {
                let value = root.get(slots.scope())?;
                ProtectedMetadata::Interface(
                    slots.scope().downcast::<InterfaceObject>(
                        value.object().ok_or(GcError::Representation)?,
                    )?,
                )
            }
            Self::Interface(location) => {
                let value = slots.get(*location)?;
                ProtectedMetadata::Interface(
                    slots.scope().downcast::<InterfaceObject>(
                        value.object().ok_or(GcError::Representation)?,
                    )?,
                )
            }
            Self::Revision { owner, source } => {
                ProtectedMetadata::Revision(source.resolve(modules, slots, owner)?)
            }
            Self::Environment(source) => ProtectedMetadata::Environment(slots.dependency(*source)?),
            Self::AdmittedEnvironment(source) => {
                ProtectedMetadata::Environment(source.resolve(slots)?)
            }
            Self::Operations(source) => ProtectedMetadata::Operations(slots.dependency(*source)?),
            Self::AdmittedOperations(source) => {
                ProtectedMetadata::Operations(source.resolve(slots)?)
            }
            Self::Application(source) => ProtectedMetadata::Application(slots.dependency(*source)?),
            Self::AdmittedApplication(source) => {
                ProtectedMetadata::Application(source.resolve(slots)?)
            }
            Self::HostEnvironment(root) => ProtectedMetadata::Environment(slots.scope().get(root)?),
            Self::HostApplication(root) => ProtectedMetadata::Application(slots.scope().get(root)?),
            Self::NativeSelections(location) => {
                let value = slots.get(*location)?;
                let object = slots
                    .scope()
                    .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
                ProtectedMetadata::Application(
                    NativeObject::selections(slots.scope(), object).ok_or(GcError::Index)?,
                )
            }
            Self::ClosureEnvironment(location) => {
                let value = slots.get(*location)?;
                let object = slots
                    .scope()
                    .downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                ProtectedMetadata::Environment(
                    ClosureObject::environment(slots.scope(), object).ok_or(GcError::Index)?,
                )
            }
        })
    }
}

pub(crate) enum MetadataError {
    Memory(GcError),
    Execution(RuntimeError),
}

impl From<GcError> for MetadataError {
    fn from(error: GcError) -> Self {
        Self::Memory(error)
    }
}

impl From<RuntimeError> for MetadataError {
    fn from(error: RuntimeError) -> Self {
        Self::Execution(error)
    }
}

impl From<TryReserveError> for MetadataError {
    fn from(_: TryReserveError) -> Self {
        Self::Memory(GcError::Allocation)
    }
}

impl Runtime {
    /// Read immutable metadata through explicitly protected roots. The callback
    /// receives no mutable heap/Runtime service; payload/GC loans cannot escape.
    pub(crate) fn with_metadata_graph<R>(
        &mut self,
        protected: &[MetadataSource<'_>],
        read: impl for<'gc> FnOnce(
            &Scope<'gc>,
            &ProtectedGraph<'gc>,
            &ResourceState,
        ) -> Result<R, MetadataError>,
    ) -> Result<R, RuntimeError> {
        self.with_metadata_slots(protected, |slots, graph, resources| {
            read(slots.scope(), graph, resources)
        })
    }

    /// Closed metadata publication may copy authenticated targets into an owned
    /// dependency suffix. The callback has no Runtime or executing service.
    pub(crate) fn with_metadata_slots<R>(
        &mut self,
        protected: &[MetadataSource<'_>],
        read: impl for<'gc> FnOnce(
            &mut Slots<'_, 'gc>,
            &ProtectedGraph<'gc>,
            &ResourceState,
        ) -> Result<R, MetadataError>,
    ) -> Result<R, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.poll_execution()?;
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::MetadataGraphAdmission);
        for (index, source) in protected.iter().enumerate() {
            if index != 0 && index.is_multiple_of(1024) {
                self.resources.poll_execution()?;
            }
            if let MetadataSource::Revision { owner, .. } = source {
                self.validate_executable_module(owner)?;
            }
        }
        let Runtime {
            memory,
            resources,
            modules,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let mut targets = Vec::new();
                targets.try_reserve_exact(protected.len())?;
                for (index, source) in protected.iter().enumerate() {
                    if index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(MetadataError::Execution(error)));
                    }
                    targets.push(source.resolve(modules, slots)?);
                }
                Ok((|| -> Result<(R, Vec<ExecutableModule>), MetadataError> {
                    let graph = ProtectedGraph::new(slots.scope(), &targets, resources)?;
                    let value = read(slots, &graph, resources)?;
                    let programs = graph.programs(slots.scope(), resources)?;
                    Ok((value, programs))
                })())
            })
        });
        let (value, programs) = result
            .map_err(|error| self.metadata_input_error(error))?
            .map_err(|error| match error {
                MetadataError::Memory(error) => self.managed_memory_error(error),
                MetadataError::Execution(error) => self.resources.admit_execution_error(error),
            })?;
        for (index, program) in programs.iter().enumerate() {
            if index.is_multiple_of(1024) {
                self.resources.poll_execution()?;
            }
            self.validate_executable_module(program)?;
        }
        Ok(value)
    }

    pub(in crate::execution_metadata) fn metadata_input_error(
        &self,
        error: GcError,
    ) -> RuntimeError {
        match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => {
                RuntimeError::module_validation("foreign, expired or mismatched metadata source")
            }
            error => self.managed_memory_error(error),
        }
    }
}
