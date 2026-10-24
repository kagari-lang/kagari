//! Runtime-owned method applications, addressed by checked identities.
use crate::{
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    frame::types::{TypeEnvironment, arguments::ScopedSignature},
    gc::{GcHeap, interfaces::InterfaceResultBinding},
    module::dispatch::SelectedTarget,
};
use kagari_types::callable::Signature;
use std::{
    cell::Ref,
    collections::{HashSet, TryReserveError},
};

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
    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        let environment = match self {
            Self::Semantic(application) => {
                if let Some(adapter) = &application.result_adapter {
                    pending.push(MetadataEdge::Binding(adapter));
                }
                &application.environment
            }
            Self::Admitted(computation) => {
                pending.push(MetadataEdge::Program(computation.selected.owner()));
                &computation.environment
            }
        };
        pending.extend(
            environment
                .iter()
                .map(|environment| MetadataEdge::Environment(environment.id)),
        );
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
    free: Vec<usize>,
    count: usize,
}

impl ApplicationStore {
    pub(crate) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            free: Vec::new(),
            count: 0,
        }
    }

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

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// The whole graph has been marked and validated before any store detaches.
    pub(crate) fn detach(&mut self, live: &HashSet<ApplicationId>) -> Vec<ApplicationRecord> {
        let mut retired = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let id = ApplicationId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            };
            if !live.contains(&id)
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
        computation: MethodComputation,
    ) -> Result<ApplicationId, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.applications
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("method application store is borrowed"))?
            .insert_record(ApplicationRecord::Admitted(computation))
            .map_err(|_| self.resource_limit("method computation storage"))
    }

    pub(crate) fn method_computation(
        &self,
        id: ApplicationId,
    ) -> Option<Ref<'_, MethodComputation>> {
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
        application: MethodApplication,
    ) -> Result<ApplicationId, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.applications
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("method application store is borrowed"))?
            .insert(application)
            .map_err(|_| self.resource_limit("method application storage"))
    }

    pub(crate) fn method_application(
        &self,
        id: ApplicationId,
    ) -> Option<Ref<'_, MethodApplication>> {
        Ref::filter_map(self.applications.try_borrow().ok()?, |store| store.get(id)).ok()
    }
}

#[cfg(test)]
mod tests;
