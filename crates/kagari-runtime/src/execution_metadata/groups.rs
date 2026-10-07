//! Runtime-owned operation groups, addressed by checked identities.
use crate::{
    error::RuntimeError,
    execution_metadata::{groups::group::OperationGroup, operation::BoundOperation},
    gc::GcHeap,
};
use std::{
    cell::Ref,
    collections::{HashSet, TryReserveError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OperationGroupId {
    owner: u64,
    slot: usize,
    generation: u64,
}

/// A member ordinal within one checked, non-renumbering operation group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OperationId {
    pub(crate) group: OperationGroupId,
    pub(crate) member: usize,
}

#[derive(Debug)]
struct Slot {
    generation: u64,
    group: Option<OperationGroup>,
}

#[derive(Debug)]
pub(crate) struct OperationGroupStore {
    owner: u64,
    slots: Vec<Slot>,
    free: Vec<usize>,
    count: usize,
}

impl OperationGroupStore {
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
        entries: Vec<BoundOperation>,
    ) -> Result<OperationGroupId, TryReserveError> {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.slots.try_reserve(1)?;
                self.slots.push(Slot {
                    generation: 0,
                    group: None,
                });
                self.slots.len() - 1
            }
        };
        let record = &mut self.slots[slot];
        let id = OperationGroupId {
            owner: self.owner,
            slot,
            generation: record.generation,
        };
        record.group = Some(OperationGroup::new(entries, id));
        self.count += 1;
        Ok(id)
    }

    pub(crate) fn get(&self, id: OperationGroupId) -> Option<&OperationGroup> {
        if id.owner != self.owner {
            return None;
        }
        let slot = self.slots.get(id.slot)?;
        (slot.generation == id.generation)
            .then_some(slot.group.as_ref())
            .flatten()
    }

    pub(crate) fn operation(&self, id: OperationId) -> Option<&BoundOperation> {
        self.get(id.group)?.get(id)
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// The whole graph has been marked and validated before any store detaches.
    pub(crate) fn detach(&mut self, live: &HashSet<OperationGroupId>) -> Vec<OperationGroup> {
        let mut retired = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let id = OperationGroupId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            };
            if !live.contains(&id)
                && let Some(group) = slot.group.take()
            {
                retired.push(group);
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
    pub(crate) fn alloc_operation_group(
        &self,
        entries: Vec<BoundOperation>,
    ) -> Result<OperationGroupId, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.operation_groups
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("operation group store is borrowed"))?
            .insert(entries)
            .map_err(|_| self.resource_limit("operation group storage"))
    }

    pub(crate) fn alloc_bound_operation(
        &self,
        operation: BoundOperation,
    ) -> Result<OperationId, RuntimeError> {
        let group = self.alloc_operation_group(vec![operation])?;
        Ok(OperationId { group, member: 0 })
    }

    pub(crate) fn bound_operation(&self, id: OperationId) -> Option<Ref<'_, BoundOperation>> {
        Ref::filter_map(self.operation_groups.try_borrow().ok()?, |store| {
            store.operation(id)
        })
        .ok()
    }

    pub(crate) fn operation_group(&self, id: OperationGroupId) -> Option<Ref<'_, OperationGroup>> {
        Ref::filter_map(self.operation_groups.try_borrow().ok()?, |store| {
            store.get(id)
        })
        .ok()
    }
}

mod group;

#[cfg(test)]
mod tests;
