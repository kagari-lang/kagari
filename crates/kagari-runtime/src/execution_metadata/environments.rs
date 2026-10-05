//! Central executable environment storage; handles retain immutable type facts only.
use crate::{
    error::RuntimeError,
    frame::types::{EnvironmentRecord, TypeEnvironment, operations::OperationBindings},
    gc::GcHeap,
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
    environment: Option<EnvironmentRecord>,
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
    pub(crate) fn insert(
        &mut self,
        environment: EnvironmentRecord,
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
    pub(crate) fn detach(&mut self, live: &HashSet<EnvironmentId>) -> Vec<EnvironmentRecord> {
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

impl GcHeap {
    pub(crate) fn alloc_environment(
        &self,
        record: EnvironmentRecord,
    ) -> Result<TypeEnvironment, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !record.validate(self) {
            return Err(RuntimeError::module_validation("invalid environment edges"));
        }
        let types = record.types.clone();
        let id = self
            .environments
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("environment store is borrowed"))?
            .insert(record)
            .map_err(|_| self.resource_limit("environment storage"))?;
        Ok(TypeEnvironment { id, types })
    }

    pub(crate) fn extend_environment(
        &self,
        environment: &TypeEnvironment,
        operations: OperationBindings,
    ) -> Result<TypeEnvironment, RuntimeError> {
        let mut record = self
            .environment(environment.id)
            .ok_or_else(|| RuntimeError::module_validation("invalid executable environment"))?
            .clone();
        record.extend_operations(operations);
        self.alloc_environment(record)
    }

    pub(crate) fn environment(&self, id: EnvironmentId) -> Option<Ref<'_, EnvironmentRecord>> {
        Ref::filter_map(self.environments.try_borrow().ok()?, |store| store.get(id)).ok()
    }
}

#[cfg(test)]
mod tests;
