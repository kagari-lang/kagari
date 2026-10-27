//! Runtime-owned method applications, addressed by checked identities.
use crate::{
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    frame::types::{TypeEnvironment, arguments::ScopedSignature},
    gc::{
        GcHeap,
        graph::{
            GraphIdentity,
            marks::{Domain, MarkBlocks, MarkRef},
        },
        interfaces::InterfaceResultBinding,
    },
    module::{ModuleStore, dispatch::SelectedTarget},
};
use kagari_types::callable::Signature;
use std::{cell::Ref, collections::TryReserveError};

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
    pub(crate) selected: SelectedTarget,
    pub(crate) environment: Option<TypeEnvironment>,
}

#[derive(Debug)]
pub(crate) enum ApplicationRecord {
    Semantic(MethodApplication),
    Admitted(MethodComputation),
}

impl ApplicationRecord {
    pub(crate) fn scan_field(&self, field: usize) -> Option<MetadataEdge<'_>> {
        match self {
            Self::Semantic(application) => match field {
                0 => application
                    .environment
                    .as_ref()
                    .map(|environment| MetadataEdge::Environment(environment.id)),
                1 | 2 => application.result_adapter.as_ref()?.scan_field(field - 1),
                _ => None,
            },
            Self::Admitted(computation) => match field {
                0 => Some(MetadataEdge::Program(computation.selected.owner())),
                1 => computation
                    .environment
                    .as_ref()
                    .map(|environment| MetadataEdge::Environment(environment.id)),
                _ => None,
            },
        }
    }

    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        match self {
            Self::Semantic(application) => {
                pending.extend(self.scan_field(0));
                pending.extend(
                    application
                        .result_adapter
                        .as_ref()
                        .map(MetadataEdge::Binding),
                );
            }
            Self::Admitted(_) => pending.extend((0..3).filter_map(|field| self.scan_field(field))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ApplicationId {
    owner: u64,
    slot: usize,
    generation: u64,
}

#[derive(Debug)]
struct Slot {
    generation: u64,
    application: Option<ApplicationRecord>,
}

#[derive(Debug)]
pub(crate) struct ApplicationStore {
    owner: u64,
    slots: Vec<Slot>,
    marks: MarkBlocks,
    free: Vec<usize>,
    count: usize,
}

impl ApplicationStore {
    pub(crate) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            marks: MarkBlocks::default(),
            free: Vec::new(),
            count: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn insert(
        &mut self,
        application: MethodApplication,
    ) -> Result<ApplicationId, TryReserveError> {
        self.insert_record(ApplicationRecord::Semantic(application))
    }

    fn insert_record(
        &mut self,
        application: ApplicationRecord,
    ) -> Result<ApplicationId, TryReserveError> {
        self.marks
            .prepare(self.free.last().copied().unwrap_or(self.slots.len()))?;
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.slots.try_reserve(1)?;
                self.slots.push(Slot {
                    generation: 0,
                    application: None,
                });
                self.slots.len() - 1
            }
        };
        let record = &mut self.slots[slot];
        let id = ApplicationId {
            owner: self.owner,
            slot,
            generation: record.generation,
        };
        record.application = Some(application);
        self.count += 1;
        Ok(id)
    }

    pub(crate) fn get(&self, id: ApplicationId) -> Option<&MethodApplication> {
        match self.record(id)? {
            ApplicationRecord::Semantic(application) => Some(application),
            ApplicationRecord::Admitted(_) => None,
        }
    }

    pub(crate) fn record(&self, id: ApplicationId) -> Option<&ApplicationRecord> {
        if id.owner != self.owner {
            return None;
        }
        let slot = self.slots.get(id.slot)?;
        (slot.generation == id.generation)
            .then_some(slot.application.as_ref())
            .flatten()
    }

    pub(crate) fn mark(&self, id: ApplicationId) -> Option<MarkRef<'_>> {
        self.record(id)?;
        Some(self.marks.slot(Domain::Application, id.slot))
    }

    pub(crate) fn next_gray(&self, block: usize, epoch: u64) -> (Option<GraphIdentity>, bool) {
        let (index, more) = self.marks.next_gray(block, epoch);
        let id = index.map(|index| {
            let slot = &self.slots[index];
            assert!(slot.application.is_some(), "gray owner is occupied");
            GraphIdentity::Application(ApplicationId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            })
        });
        (id, more)
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// The whole graph has been marked and validated before any store detaches.
    pub(crate) fn detach_batch(
        &mut self,
        cursor: &mut usize,
        limit: usize,
        mut survivor: impl FnMut(MarkRef<'_>) -> bool,
    ) -> Vec<ApplicationRecord> {
        assert!(limit != 0);
        let mut retired = Vec::new();
        while *cursor < self.slots.len() && retired.len() < limit {
            let index = *cursor;
            *cursor += 1;
            let slot = &mut self.slots[index];
            if !survivor(self.marks.slot(Domain::Application, index))
                && let Some(application) = slot.application.take()
            {
                retired.push(application);
                self.count -= 1;
                if let Some(generation) = slot.generation.checked_add(1) {
                    slot.generation = generation;
                    self.free.push(index);
                }
            }
        }
        retired
    }
}

impl GcHeap {
    pub(crate) fn alloc_method_computation(
        &self,
        modules: &ModuleStore,
        computation: MethodComputation,
    ) -> Result<ApplicationId, RuntimeError> {
        self.alloc_application_record(modules, ApplicationRecord::Admitted(computation))
    }

    fn alloc_application_record(
        &self,
        modules: &ModuleStore,
        record: ApplicationRecord,
    ) -> Result<ApplicationId, RuntimeError> {
        self.ensure_execution_allowed()?;
        for field in 0..3 {
            if let Some(edge) = record.scan_field(field) {
                self.publish_metadata_edge(modules, edge)?;
            }
        }
        let mut graph = self
            .graph
            .try_lock()
            .map_err(|_| RuntimeError::module_validation("graph admission is borrowed"))?;
        graph.prepare_identity()?;
        let id = self
            .applications
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("method application store is borrowed"))?
            .insert_record(record)
            .map_err(|_| self.resource_limit("method computation storage"))?;
        self.with_heap_mark(GraphIdentity::Application(id), |mark| {
            graph.admit_prepared(GraphIdentity::Application(id), mark);
            Ok(())
        })?;
        Ok(id)
    }

    pub(crate) fn method_computation(
        &self,
        id: ApplicationId,
    ) -> Option<Ref<'_, MethodComputation>> {
        if !self.heap_identity_accessible(GraphIdentity::Application(id)) {
            return None;
        }
        Ref::filter_map(self.applications.try_borrow().ok()?, |store| {
            match store.record(id)? {
                ApplicationRecord::Admitted(computation) => Some(computation),
                ApplicationRecord::Semantic(_) => None,
            }
        })
        .ok()
    }

    pub(crate) fn alloc_method_application(
        &self,
        modules: &ModuleStore,
        application: MethodApplication,
    ) -> Result<ApplicationId, RuntimeError> {
        self.alloc_application_record(modules, ApplicationRecord::Semantic(application))
    }

    pub(crate) fn method_application(
        &self,
        id: ApplicationId,
    ) -> Option<Ref<'_, MethodApplication>> {
        if !self.heap_identity_accessible(GraphIdentity::Application(id)) {
            return None;
        }
        Ref::filter_map(self.applications.try_borrow().ok()?, |store| store.get(id)).ok()
    }
}

#[cfg(test)]
mod tests;
