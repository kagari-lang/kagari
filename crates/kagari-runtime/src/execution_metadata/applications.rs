//! Semantic applications and exact computations have execution-owned providers.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{DependencyId, ExecutionRoots, LocalMark, SessionId, Slots},
    execution_metadata::{
        MetadataEdge,
        identity::MetadataIdentity,
        managed::{
            ProtectedMetadata,
            applications::ApplicationObject,
            sources::{MetadataError, MetadataSource},
        },
    },
    frame::types::{TypeEnvironment, arguments::ScopedSignature},
    module::dispatch::SelectedTarget,
    native::{application::NativeApplication, stored_selection::StoredSelection},
    objects::{interfaces::description::InterfaceResultBinding, method_view::MethodFacts},
};
use kagari_gc::{error::GcError, object::Gc, root::Root};
use kagari_types::callable::Signature;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct MethodApplication {
    pub(crate) signature: Signature,
    pub(crate) scoped_signature: Option<ScopedSignature>,
    /// Applied entry facts in the relationship's publication, not a code selection.
    /// MethodView transports these immutable facts for each admitted computation.
    pub(crate) environment: Option<TypeEnvironment>,
    pub(crate) result_adapter: Option<InterfaceResultBinding>,
}

/// An admitted computation, separate from reusable semantic application facts.
/// Its owner publishes this graph edge before enqueueing or entering execution.
#[derive(Debug, Clone)]
pub(crate) struct MethodComputation {
    pub(crate) relationship: MethodFacts,
    pub(crate) selected: SelectedTarget,
    pub(crate) environment: Option<TypeEnvironment>,
}

#[derive(Debug)]
pub(crate) enum ApplicationRecord {
    Semantic(MethodApplication),
    Admitted(MethodComputation),
    Native(Arc<NativeApplication>),
    StoredSelections(Arc<[StoredSelection]>),
}

impl ApplicationRecord {
    pub(crate) fn trace<'a, E, F>(&'a self, visit: &mut F) -> Result<(), E>
    where
        F: FnMut(MetadataEdge<'a>) -> Result<(), E> + ?Sized,
    {
        let environment = match self {
            Self::Semantic(application) => {
                if let Some(adapter) = &application.result_adapter {
                    visit(MetadataEdge::Binding(adapter))?;
                }
                &application.environment
            }
            Self::Admitted(computation) => {
                computation.relationship.trace(visit)?;
                visit(MetadataEdge::Program(computation.selected.owner()))?;
                &computation.environment
            }
            Self::Native(application) => return application.trace(visit),
            Self::StoredSelections(selections) => {
                for selection in selections.iter() {
                    selection.trace(visit)?;
                }
                return Ok(());
            }
        };
        if let Some(environment) = environment {
            visit(MetadataEdge::EnvironmentFacts(environment))?;
        }
        Ok(())
    }
}

/// A never-reused semantic identity, not a managed-state locator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ApplicationId(MetadataIdentity);

/// Application facts and their actual execution-owned source. Keeping this
/// descriptor after retirement preserves facts, never access to managed state.
#[derive(Debug, Clone)]
pub(crate) struct AdmittedApplication {
    id: ApplicationId,
    record: Arc<ApplicationRecord>,
    source: DependencyId<ApplicationObject>,
}

impl AdmittedApplication {
    pub(crate) fn publish<'gc>(
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
        value: Gc<'gc, ApplicationObject>,
    ) -> Result<Self, GcError> {
        let (id, record) = slots
            .scope()
            .read(value, |value| (value.id(), value.record().clone()));
        let source = slots.push_dependency(execution, value)?;
        Ok(Self { id, record, source })
    }

    pub(crate) fn preserve<'gc>(
        slots: &mut Slots<'_, 'gc>,
        mark: &mut LocalMark,
        expected: &Self,
    ) -> Result<Self, GcError> {
        let value = expected.resolve(slots)?;
        let source = slots.preserve_dependency(mark, value)?;
        Ok(Self {
            id: expected.id,
            record: expected.record.clone(),
            source,
        })
    }

    pub(crate) fn id(&self) -> ApplicationId {
        self.id
    }

    pub(crate) fn record(&self) -> &Arc<ApplicationRecord> {
        &self.record
    }

    pub(crate) fn source(&self) -> DependencyId<ApplicationObject> {
        self.source
    }

    pub(crate) fn resolve<'gc>(
        &self,
        slots: &mut Slots<'_, 'gc>,
    ) -> Result<Gc<'gc, ApplicationObject>, GcError> {
        let value = slots.dependency(self.source)?;
        if !slots.scope().read(value, |value| {
            value.id() == self.id && Arc::ptr_eq(value.record(), &self.record)
        }) {
            return Err(GcError::Representation);
        }
        Ok(value)
    }
}

impl Runtime {
    /// Copy one authenticated application into a destination preparation suffix.
    /// A missing optional cache target does not recover state from its ID.
    pub(crate) fn copy_application_in(
        &mut self,
        execution: SessionId,
        id: ApplicationId,
        protected: &[MetadataSource<'_>],
    ) -> Result<Option<AdmittedApplication>, RuntimeError> {
        self.with_metadata_slots(protected, |slots, graph, resources| {
            let Some(ProtectedMetadata::Application(value)) =
                graph.find(slots.scope(), MetadataEdge::Application(id), resources)?
            else {
                return Ok(None);
            };
            let record = slots.scope().read(value, |value| value.record().clone());
            let source = slots.push_dependency(execution, value)?;
            Ok(Some(AdmittedApplication { id, record, source }))
        })
    }

    pub(crate) fn application_in(
        &mut self,
        execution: SessionId,
        id: ApplicationId,
        protected: &[MetadataSource<'_>],
    ) -> Result<AdmittedApplication, RuntimeError> {
        self.copy_application_in(execution, id, protected)?
            .ok_or_else(|| RuntimeError::module_validation("application is not protected"))
    }

    pub(crate) fn retain_application(
        &mut self,
        application: &AdmittedApplication,
    ) -> Result<Root<ApplicationObject>, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        let result = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = application.resolve(slots)?;
                slots.scope().retain(value)
            })
        });
        result.map_err(|error| self.metadata_input_error(error))
    }

    /// Publish semantic application facts or an exact admitted computation into
    /// the destination preparation/frame/Native dependency suffix. Every required
    /// environment/provider/body is resolved from the supplied actual sources.
    pub(crate) fn alloc_application(
        &mut self,
        execution: SessionId,
        record: ApplicationRecord,
        protected: &[MetadataSource<'_>],
    ) -> Result<AdmittedApplication, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.poll_execution()?;
        // Storage retention does not authorize revoked transitive providers.
        // Finish admission with no source/payload loan before allocating a node.
        self.with_metadata_graph(protected, |_, _, _| Ok(()))?;
        match &record {
            ApplicationRecord::Admitted(computation) => {
                self.validate_executable_module(computation.selected.owner())?
            }
            ApplicationRecord::Semantic(application) => {
                if let Some(adapter) = &application.result_adapter {
                    self.validate_executable_module(&adapter.owner)?;
                }
            }
            ApplicationRecord::Native(application) => {
                self.validate_executable_module(&application.owner)?
            }
            ApplicationRecord::StoredSelections(selections) => {
                for (index, selection) in selections.iter().enumerate() {
                    if index.is_multiple_of(1024) {
                        self.resources.poll_execution()?;
                    }
                    self.validate_executable_module(selection.owner())?;
                }
            }
        }
        let Runtime {
            memory,
            modules,
            metadata_ids,
            resources,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<AdmittedApplication, MetadataError> {
                    slots.storage(execution)?;
                    let mut targets = Vec::new();
                    targets.try_reserve_exact(protected.len())?;
                    for (index, source) in protected.iter().enumerate() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        targets.push(source.resolve(modules, slots)?);
                    }
                    let id = ApplicationId(metadata_ids.fresh()?);
                    let record = Arc::new(record);
                    let value = ApplicationObject::alloc(
                        slots.scope(),
                        id,
                        record.clone(),
                        &targets,
                        resources,
                    )?;
                    let source = slots.push_dependency(execution, value)?;
                    Ok(AdmittedApplication { id, record, source })
                })())
            })
        });
        result
            .map_err(|error| self.metadata_input_error(error))?
            .map_err(|error| match error {
                MetadataError::Memory(error) => self.metadata_input_error(error),
                MetadataError::Execution(error) => self.resources.admit_execution_error(error),
            })
    }

    #[cfg(test)]
    pub(crate) fn application_record(
        &mut self,
        application: &AdmittedApplication,
    ) -> Result<Arc<ApplicationRecord>, RuntimeError> {
        self.with_metadata_graph(
            &[MetadataSource::AdmittedApplication(application)],
            |cx, graph, _| {
                let Some(ProtectedMetadata::Application(value)) = graph.single_root() else {
                    return Err(GcError::Representation.into());
                };
                Ok(cx.read(value, |value| value.record().clone()))
            },
        )
    }

    /// Select only inside the explicitly protected actual graph. The ID alone
    /// cannot recover a managed application or authorize execution.
    pub(crate) fn application_record_in(
        &mut self,
        protected: &[MetadataSource<'_>],
        id: ApplicationId,
    ) -> Result<Arc<ApplicationRecord>, RuntimeError> {
        self.with_metadata_graph(protected, |cx, graph, resources| {
            let Some(ProtectedMetadata::Application(value)) =
                graph.find(cx, MetadataEdge::Application(id), resources)?
            else {
                return Err(RuntimeError::module_validation("application is not protected").into());
            };
            Ok(cx.read(value, |value| value.record().clone()))
        })
    }
}

#[cfg(test)]
mod tests;
