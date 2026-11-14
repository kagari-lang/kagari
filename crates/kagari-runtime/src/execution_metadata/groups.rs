//! Operation dictionaries publish directly into their execution's managed graph.
pub(crate) mod group;
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{DependencyId, ExecutionRoots, SessionId, Slots},
    execution_metadata::{
        MetadataEdge,
        groups::group::OperationGroup,
        identity::MetadataIdentity,
        managed::{
            ProtectedMetadata,
            groups::OperationGroupObject,
            sources::{MetadataError, MetadataSource},
        },
        operation::BoundOperation,
    },
};
use kagari_gc::{error::GcError, object::Gc};
use std::sync::Arc;

/// A never-reused semantic identity; it cannot reopen a dictionary by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OperationGroupId(MetadataIdentity);

/// A member ordinal within one immutable operation dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OperationId {
    pub(crate) group: OperationGroupId,
    pub(crate) member: usize,
}

/// Immutable dictionary facts paired with their actual execution-owned source.
/// Cloning this handle does not retain managed state after source retirement.
#[derive(Debug, Clone)]
pub(crate) struct AdmittedOperationGroup {
    record: Arc<OperationGroup>,
    source: DependencyId<OperationGroupObject>,
}

impl AdmittedOperationGroup {
    pub(crate) fn publish<'gc>(
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
        value: Gc<'gc, OperationGroupObject>,
    ) -> Result<Self, GcError> {
        let record = slots.scope().read(value, |value| value.record().clone());
        let source = slots.push_dependency(execution, value)?;
        Ok(Self { record, source })
    }

    pub(crate) fn record(&self) -> &Arc<OperationGroup> {
        &self.record
    }

    pub(crate) fn source(&self) -> DependencyId<OperationGroupObject> {
        self.source
    }

    pub(crate) fn resolve<'gc>(
        &self,
        slots: &mut Slots<'_, 'gc>,
    ) -> Result<Gc<'gc, OperationGroupObject>, GcError> {
        let value = slots.dependency(self.source)?;
        if !slots
            .scope()
            .read(value, |value| Arc::ptr_eq(value.record(), &self.record))
        {
            return Err(GcError::Representation);
        }
        Ok(value)
    }
}

impl Runtime {
    /// Copy an actual dictionary selected inside the caller's fixed graph.
    /// The semantic ID selects among those targets and cannot recover one alone.
    pub(crate) fn operation_group_in(
        &mut self,
        execution: SessionId,
        id: OperationGroupId,
        protected: &[MetadataSource<'_>],
    ) -> Result<AdmittedOperationGroup, RuntimeError> {
        self.with_metadata_slots(protected, |slots, graph, resources| {
            let Some(ProtectedMetadata::Operations(group)) =
                graph.find(slots.scope(), MetadataEdge::Group(id), resources)?
            else {
                return Err(RuntimeError::module_validation(
                    "operation dictionary is not protected",
                )
                .into());
            };
            Ok(AdmittedOperationGroup::publish(slots, execution, group)?)
        })
    }

    /// Allocate one dictionary and publish it into the destination execution's
    /// dependency suffix. Its preparation/frame/Native owner must retire it.
    /// Sources are authenticated before any fallible construction or slot growth.
    pub(crate) fn alloc_operation_group(
        &mut self,
        execution: SessionId,
        entries: Vec<BoundOperation>,
        protected: &[MetadataSource<'_>],
    ) -> Result<AdmittedOperationGroup, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.poll_execution()?;
        // Storage retention does not authorize revoked transitive providers.
        // Finish admission with no source/payload loan before allocating a node.
        self.with_metadata_graph(protected, |_, _, _| Ok(()))?;
        // Revoked code cannot become callable merely because an old revision's
        // managed allocation still survives. Check availability before publishing.
        for (index, operation) in entries.iter().enumerate() {
            if index.is_multiple_of(1024) {
                self.resources.poll_execution()?;
            }
            self.validate_executable_module(&operation.owner)?;
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
                Ok((|| -> Result<AdmittedOperationGroup, MetadataError> {
                    slots.storage(execution)?;
                    let mut targets = Vec::new();
                    targets.try_reserve_exact(protected.len())?;
                    for (index, source) in protected.iter().enumerate() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        targets.push(source.resolve(modules, slots)?);
                    }
                    let id = OperationGroupId(metadata_ids.fresh()?);
                    let record = Arc::new(OperationGroup::new(entries, id, resources)?);
                    let value = OperationGroupObject::alloc(
                        slots.scope(),
                        record.clone(),
                        &targets,
                        resources,
                    )?;
                    let source = slots.push_dependency(execution, value)?;
                    Ok(AdmittedOperationGroup { record, source })
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

    /// Read pure facts only after authenticating the actual source and descriptor.
    pub(crate) fn operation_group_record(
        &mut self,
        group: &AdmittedOperationGroup,
    ) -> Result<Arc<OperationGroup>, RuntimeError> {
        self.with_metadata_graph(
            &[MetadataSource::AdmittedOperations(group)],
            |cx, graph, _| {
                let Some(ProtectedMetadata::Operations(value)) = graph.single_root() else {
                    return Err(GcError::Representation.into());
                };
                Ok(cx.read(value, |value| value.record().clone()))
            },
        )
    }
}

#[cfg(test)]
mod tests;
