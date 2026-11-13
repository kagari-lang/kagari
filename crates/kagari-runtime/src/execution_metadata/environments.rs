//! Environment identities name facts; execution owns actual managed sources.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
#[cfg(test)]
use crate::execution_metadata::managed::ProtectedMetadata;
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{DependencyId, ExecutionRoots, SessionId, Slots},
    execution_metadata::{
        groups::{OperationGroupId, OperationId, group::OperationGroup},
        identity::MetadataIdentity,
        managed::{
            ProtectedGraph,
            environments::EnvironmentObject,
            sources::{MetadataError, MetadataSource},
        },
    },
    frame::types::{EnvironmentRecord, TypeEnvironment, compatibility::TypeIdentity},
};
use kagari_gc::{error::GcError, object::Gc, root::Root};
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::NominalTy};
use std::{collections::HashMap, sync::Arc};

/// A never-reused semantic identity, not a table locator or heap capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EnvironmentId(MetadataIdentity);

/// An execution-owned source plus immutable lexical facts. Copying facts alone
/// cannot retain or reopen the environment after the source suffix is retired.
#[derive(Debug, Clone)]
pub(crate) struct AdmittedEnvironment {
    facts: TypeEnvironment,
    source: DependencyId<EnvironmentObject>,
}

impl AdmittedEnvironment {
    pub(crate) fn facts(&self) -> &TypeEnvironment {
        &self.facts
    }

    pub(crate) fn source(&self) -> DependencyId<EnvironmentObject> {
        self.source
    }

    /// Publish an already authenticated actual node into a new owning suffix.
    /// Read facts from that same node; an unrelated ID/facts pair is never enough.
    pub(crate) fn publish<'gc>(
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
        value: Gc<'gc, EnvironmentObject>,
    ) -> Result<Self, GcError> {
        let facts = slots.scope().read(value, |value| TypeEnvironment {
            id: value.id(),
            types: value.record().types.clone(),
        });
        let source = slots.push_dependency(execution, value)?;
        Ok(Self { facts, source })
    }

    pub(in crate::execution_metadata) fn resolve<'gc>(
        &self,
        slots: &mut Slots<'_, 'gc>,
    ) -> Result<Gc<'gc, EnvironmentObject>, GcError> {
        let value = slots.dependency(self.source)?;
        if !slots.scope().read(value, |value| {
            value.id() == self.facts.id && Arc::ptr_eq(&value.record().types, &self.facts.types)
        }) {
            return Err(GcError::Representation);
        }
        Ok(value)
    }
}

/// Pure transport facts from one authenticated graph read. This snapshot owns
/// no managed state; a derived node still requires its original actual sources.
pub(crate) struct EnvironmentSnapshot {
    pub(crate) record: Arc<EnvironmentRecord>,
    pub(crate) groups: HashMap<OperationGroupId, Arc<OperationGroup>>,
}

impl EnvironmentSnapshot {
    pub(crate) fn operation_slot(
        &self,
        receiver: &TypeIdentity,
        interface: &NominalTy,
        slot: u32,
    ) -> Option<OperationId> {
        self.record
            .operations()
            .operation_slot(&self.groups, receiver, interface, slot)
    }

    pub(crate) fn operation(
        &self,
        required: &NativeCallableRequirement,
        receiver: &TypeIdentity,
    ) -> Option<OperationId> {
        self.record
            .operations()
            .operation(&self.groups, required, receiver)
    }
}

impl Runtime {
    /// Promote actual admitted state for an escaping Host owner. A root retains
    /// the graph; code/installation admission remains the consuming call's duty.
    pub(crate) fn retain_environment(
        &mut self,
        environment: &AdmittedEnvironment,
    ) -> Result<Root<EnvironmentObject>, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        let result = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = environment.resolve(slots)?;
                slots.scope().retain(value)
            })
        });
        result.map_err(|error| self.environment_input_error(error))
    }

    /// Publish a complete environment directly into the supplied execution's
    /// dependency suffix. Its frame/Native/preparation owner retires that suffix.
    /// Original sources are admitted before any allocation can reuse a stale slot.
    pub(crate) fn alloc_environment(
        &mut self,
        execution: SessionId,
        record: EnvironmentRecord,
        protected: &[MetadataSource<'_>],
    ) -> Result<AdmittedEnvironment, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::EnvironmentAllocation);
        self.resources.ensure_synchronous_thread()?;
        self.resources.poll_execution()?;
        // Storage retention does not authorize revoked transitive providers.
        // Finish admission with no source/payload loan before allocating a node.
        self.with_metadata_graph(protected, |_, _, _| Ok(()))?;
        let Runtime {
            memory,
            modules,
            metadata_ids,
            resources,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<AdmittedEnvironment, MetadataError> {
                    slots.storage(execution)?;
                    let mut targets = Vec::new();
                    targets.try_reserve_exact(protected.len())?;
                    for (index, source) in protected.iter().enumerate() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        targets.push(source.resolve(modules, slots)?);
                    }
                    let record = Arc::new(record);
                    let id = EnvironmentId(metadata_ids.fresh()?);
                    let facts = TypeEnvironment {
                        id,
                        types: record.types.clone(),
                    };
                    let publish = || -> Result<DependencyId<EnvironmentObject>, MetadataError> {
                        let value = EnvironmentObject::alloc(
                            slots.scope(),
                            id,
                            record,
                            &targets,
                            resources,
                        )?;
                        Ok(slots.push_dependency(execution, value)?)
                    };
                    let source = publish()?;
                    Ok(AdmittedEnvironment { facts, source })
                })())
            })
        });
        result
            .map_err(|error| self.environment_input_error(error))?
            .map_err(|error| match error {
                MetadataError::Memory(error) => self.environment_input_error(error),
                MetadataError::Execution(error) => self.resources.admit_execution_error(error),
            })
    }

    #[cfg(test)]
    pub(crate) fn environment_record(
        &mut self,
        environment: &AdmittedEnvironment,
    ) -> Result<Arc<EnvironmentRecord>, RuntimeError> {
        self.with_metadata_graph(
            &[MetadataSource::AdmittedEnvironment(environment)],
            |cx, graph, _| {
                let Some(ProtectedMetadata::Environment(value)) = graph.single_root() else {
                    return Err(GcError::Representation.into());
                };
                Ok(cx.read(value, |value| value.record().clone()))
            },
        )
    }

    pub(crate) fn environment_snapshot(
        &mut self,
        environment: &AdmittedEnvironment,
    ) -> Result<EnvironmentSnapshot, RuntimeError> {
        self.environment_snapshot_in(
            &[MetadataSource::AdmittedEnvironment(environment)],
            environment.facts(),
        )
    }

    /// Select facts only within the supplied actual graph. A bare environment
    /// identity is insufficient: the graph and exact type descriptor must match.
    pub(crate) fn environment_snapshot_in(
        &mut self,
        protected: &[MetadataSource<'_>],
        environment: &TypeEnvironment,
    ) -> Result<EnvironmentSnapshot, RuntimeError> {
        self.with_metadata_graph(protected, |cx, graph, resources| {
            let Some(value) = graph.environment(cx, environment, resources)? else {
                return Err(RuntimeError::module_validation("environment is not protected").into());
            };
            Ok(EnvironmentSnapshot {
                record: cx.read(value, |value| value.record().clone()),
                groups: graph.operation_groups(cx, resources)?,
            })
        })
    }

    /// Copy an environment from explicitly protected nodes into the destination
    /// suffix. No object or Host root is allocated. Missing optional cache backing
    /// returns None; invalid original sources and mismatched facts remain errors.
    pub(crate) fn copy_environment_in(
        &mut self,
        execution: SessionId,
        expected: &TypeEnvironment,
        protected: &[MetadataSource<'_>],
    ) -> Result<Option<AdmittedEnvironment>, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.poll_execution()?;
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
            modules,
            resources,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                slots.storage(execution)?;
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
                Ok((|| -> Result<_, MetadataError> {
                    let graph = ProtectedGraph::new(slots.scope(), &targets, resources)?;
                    let target = graph.environment(slots.scope(), expected, resources)?;
                    let programs = graph.programs(slots.scope(), resources)?;
                    let environment = target
                        .map(|value| {
                            Ok::<_, GcError>(AdmittedEnvironment {
                                facts: expected.clone(),
                                source: slots.push_dependency(execution, value)?,
                            })
                        })
                        .transpose()?;
                    Ok((environment, programs))
                })())
            })
        });
        let (environment, programs) = result
            .map_err(|error| self.environment_input_error(error))?
            .map_err(|error| match error {
                MetadataError::Memory(error) => self.managed_memory_error(error),
                MetadataError::Execution(error) => self.resources.admit_execution_error(error),
            })?;
        for (index, owner) in programs.iter().enumerate() {
            if index.is_multiple_of(1024) {
                self.resources.poll_execution()?;
            }
            self.validate_executable_module(owner)?;
        }
        Ok(environment)
    }

    pub(crate) fn validate_environment(
        &mut self,
        environment: &AdmittedEnvironment,
    ) -> Result<(), RuntimeError> {
        self.validate_environment_in(
            &[MetadataSource::AdmittedEnvironment(environment)],
            environment.facts(),
        )
    }

    /// Authenticate facts and program availability without preparing dictionary
    /// snapshots for a caller that only needs admission.
    pub(crate) fn validate_environment_in(
        &mut self,
        protected: &[MetadataSource<'_>],
        expected: &TypeEnvironment,
    ) -> Result<(), RuntimeError> {
        self.with_metadata_graph(protected, |cx, graph, resources| {
            graph
                .environment(cx, expected, resources)?
                .ok_or_else(|| RuntimeError::module_validation("environment is not protected"))?;
            Ok(())
        })
    }

    fn environment_input_error(&self, error: GcError) -> RuntimeError {
        match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => {
                RuntimeError::module_validation("foreign, expired or mismatched environment source")
            }
            error => self.managed_memory_error(error),
        }
    }
}

#[cfg(test)]
mod tests;
