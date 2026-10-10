//! Central executable environment storage; handles retain immutable type facts only.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    frame::types::{EnvironmentRecord, TypeEnvironment},
    gc::GcHeap,
    module::LoadedModule,
};
use std::{
    cell::Ref,
    collections::{HashSet, TryReserveError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EnvironmentId {
    owner: u64,
    slot: usize,
    generation: u64,
}

#[derive(Debug)]
struct Slot {
    generation: u64,
    environment: Option<PublishedEnvironment>,
}

/// Publication proves the immutable executable graph. Dependencies are availability
/// checks, not roots; collection still traces the record's original edges.
#[derive(Debug)]
pub(crate) struct PublishedEnvironment {
    record: EnvironmentRecord,
    dependencies: Vec<LoadedModule>,
}

#[derive(Debug)]
pub(crate) struct EnvironmentStore {
    owner: u64,
    slots: Vec<Slot>,
    free: Vec<usize>,
    count: usize,
}

impl EnvironmentStore {
    pub(crate) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            free: Vec::new(),
            count: 0,
        }
    }

    fn insert(
        &mut self,
        environment: PublishedEnvironment,
    ) -> Result<EnvironmentId, TryReserveError> {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.slots.try_reserve(1)?;
                self.slots.push(Slot {
                    generation: 0,
                    environment: None,
                });
                self.slots.len() - 1
            }
        };
        let record = &mut self.slots[slot];
        let id = EnvironmentId {
            owner: self.owner,
            slot,
            generation: record.generation,
        };
        record.environment = Some(environment);
        self.count += 1;
        Ok(id)
    }

    pub(crate) fn get(&self, id: EnvironmentId) -> Option<&EnvironmentRecord> {
        Some(&self.published(id)?.record)
    }

    fn published(&self, id: EnvironmentId) -> Option<&PublishedEnvironment> {
        if id.owner != self.owner {
            return None;
        }
        let slot = self.slots.get(id.slot)?;
        (slot.generation == id.generation)
            .then_some(slot.environment.as_ref())
            .flatten()
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// The whole graph has been marked and validated before any store detaches.
    pub(crate) fn detach(&mut self, live: &HashSet<EnvironmentId>) -> Vec<PublishedEnvironment> {
        let mut retired = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let id = EnvironmentId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            };
            if !live.contains(&id)
                && let Some(environment) = slot.environment.take()
            {
                retired.push(environment);
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

impl Runtime {
    pub(crate) fn alloc_environment(
        &self,
        record: EnvironmentRecord,
    ) -> Result<TypeEnvironment, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::EnvironmentAllocation);
        self.gc.ensure_execution_allowed()?;
        // Validate before installing any identity. Environment, group and operation
        // edges are immutable; extensions must publish a new environment record.
        let dependencies = self.metadata_dependencies(MetadataEdge::EnvironmentView(&record))?;
        let types = record.types.clone();
        let id = self
            .gc
            .environments
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("environment store is borrowed"))?
            .insert(PublishedEnvironment {
                record,
                dependencies,
            })
            .map_err(|_| self.gc.resource_limit("environment storage"))?;
        Ok(TypeEnvironment { id, types })
    }

    pub(crate) fn validate_environment(&self, id: EnvironmentId) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        let environments = self
            .gc
            .environments
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("environment store is borrowed"))?;
        let published = environments
            .published(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid execution environment"))?;
        // Slot/generation admission proves the published graph is still present:
        // GC validates and detaches the whole graph atomically. Candidate program
        // leases can expire independently, so availability is checked on every entry.
        for owner in &published.dependencies {
            self.validate_loaded_module(owner)?;
        }
        Ok(())
    }
}

impl GcHeap {
    pub(crate) fn environment(&self, id: EnvironmentId) -> Option<Ref<'_, EnvironmentRecord>> {
        Ref::filter_map(self.environments.try_borrow().ok()?, |store| store.get(id)).ok()
    }
}

#[cfg(test)]
mod tests;
