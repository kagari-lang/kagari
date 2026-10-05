//! Runtime-owned interface snapshots, addressed by checked identities.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, interfaces::InterfaceValueSnapshot},
};
use std::{
    cell::Ref,
    collections::{HashSet, TryReserveError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InterfaceSnapshotId {
    owner: u64,
    slot: usize,
    generation: u64,
}

#[derive(Debug)]
struct Slot {
    generation: u64,
    interface: Option<InterfaceValueSnapshot>,
}

#[derive(Debug)]
pub(crate) struct InterfaceStore {
    owner: u64,
    slots: Vec<Slot>,
    free: Vec<usize>,
    count: usize,
}

impl InterfaceStore {
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
        interface: InterfaceValueSnapshot,
    ) -> Result<InterfaceSnapshotId, TryReserveError> {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.slots.try_reserve(1)?;
                self.slots.push(Slot {
                    generation: 0,
                    interface: None,
                });
                self.slots.len() - 1
            }
        };
        let record = &mut self.slots[slot];
        let id = InterfaceSnapshotId {
            owner: self.owner,
            slot,
            generation: record.generation,
        };
        record.interface = Some(interface);
        self.count += 1;
        Ok(id)
    }

    pub(crate) fn get(&self, id: InterfaceSnapshotId) -> Option<&InterfaceValueSnapshot> {
        if id.owner != self.owner {
            return None;
        }
        let slot = self.slots.get(id.slot)?;
        (slot.generation == id.generation)
            .then_some(slot.interface.as_ref())
            .flatten()
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// The whole graph has been marked and validated before any store detaches.
    pub(crate) fn detach(
        &mut self,
        live: &HashSet<InterfaceSnapshotId>,
    ) -> Vec<InterfaceValueSnapshot> {
        let mut retired = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let id = InterfaceSnapshotId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            };
            if !live.contains(&id)
                && let Some(interface) = slot.interface.take()
            {
                retired.push(interface);
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
    pub(crate) fn alloc_interface_snapshot(
        &self,
        interface: InterfaceValueSnapshot,
    ) -> Result<InterfaceSnapshotId, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.interfaces
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("interface snapshot store is borrowed"))?
            .insert(interface)
            .map_err(|_| self.resource_limit("interface snapshot storage"))
    }

    pub(crate) fn interface_metadata(
        &self,
        id: InterfaceSnapshotId,
    ) -> Option<Ref<'_, InterfaceValueSnapshot>> {
        Ref::filter_map(self.interfaces.try_borrow().ok()?, |store| store.get(id)).ok()
    }
}

#[cfg(test)]
mod tests;
