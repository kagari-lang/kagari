//! Runtime-owned interface snapshots, addressed by checked identities.
use crate::{
    error::RuntimeError,
    frame::types::{compatibility::TypeIdentity, construction::ConstructionIdentity},
    gc::{
        GcHeap,
        graph::{
            GraphIdentity,
            marks::{Domain, MarkBlocks, MarkRef},
        },
        interfaces::{InterfaceDescription, InterfaceValueSnapshot},
    },
    module::ModuleKey,
};
use std::{
    cell::Ref,
    collections::{HashMap, TryReserveError},
    sync::{Arc, Weak},
};

#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::storage::BufferUsage;

/// Equality preserves applied type provenance and ordered construction witnesses.
/// This runtime-local registry holds no executable roots or receivers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct InterfaceDescriptionKey {
    pub(crate) owner: ModuleKey,
    pub(crate) table: usize,
    pub(crate) view: bool,
    pub(crate) arguments: Vec<Arc<TypeIdentity>>,
    pub(crate) constructions: ConstructionIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InterfaceSnapshotId {
    owner: u64,
    slot: usize,
    generation: u64,
}

impl InterfaceSnapshotId {
    /// The compact locator is usable only after this full token is admitted by
    /// its store. An unrepresentable slot cannot publish a heap wrapper.
    pub(crate) fn directory_index(self) -> Option<u32> {
        u32::try_from(self.slot).ok()
    }
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
    marks: MarkBlocks,
    free: Vec<usize>,
    count: usize,
    descriptions: HashMap<InterfaceDescriptionKey, Weak<InterfaceDescription>>,
}

impl InterfaceStore {
    pub(crate) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            marks: MarkBlocks::default(),
            free: Vec::new(),
            count: 0,
            descriptions: HashMap::new(),
        }
    }

    pub(crate) fn insert(
        &mut self,
        interface: InterfaceValueSnapshot,
    ) -> Result<InterfaceSnapshotId, TryReserveError> {
        self.marks
            .prepare(self.free.last().copied().unwrap_or(self.slots.len()))?;
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

    /// A published directory binding cannot outlive this slot: live wrappers mark
    /// metadata, and collection invalidates dead heap IDs before detaching snapshots.
    /// Independent metadata tokens still validate their complete owner/generation.
    pub(crate) fn directory_binding(&self, index: u32) -> Option<InterfaceSnapshotId> {
        let slot = self.slots.get(index as usize)?;
        slot.interface.as_ref()?;
        Some(InterfaceSnapshotId {
            owner: self.owner,
            slot: index as usize,
            generation: slot.generation,
        })
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn storage_usage(&self) -> BufferUsage {
        BufferUsage::new("interface_snapshots", &self.slots)
    }

    pub(crate) fn prune_descriptions(&mut self) {
        self.descriptions
            .retain(|_, description| description.strong_count() != 0);
    }

    pub(crate) fn mark(&self, id: InterfaceSnapshotId) -> Option<MarkRef<'_>> {
        self.get(id)?;
        Some(self.marks.slot(Domain::Interface, id.slot))
    }

    pub(crate) fn next_gray(&self, block: usize, epoch: u64) -> (Option<GraphIdentity>, bool) {
        let (index, more) = self.marks.next_gray(block, epoch);
        let id = index.map(|index| {
            let slot = &self.slots[index];
            assert!(slot.interface.is_some(), "gray owner is occupied");
            GraphIdentity::Interface(InterfaceSnapshotId {
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
    ) -> Vec<InterfaceValueSnapshot> {
        assert!(limit != 0);
        let mut retired = Vec::new();
        while *cursor < self.slots.len() && retired.len() < limit {
            let index = *cursor;
            *cursor += 1;
            let slot = &mut self.slots[index];
            if !survivor(self.marks.slot(Domain::Interface, index))
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
    pub(crate) fn interface_description(
        &self,
        key: &InterfaceDescriptionKey,
    ) -> Option<Arc<InterfaceDescription>> {
        let description = self
            .interfaces
            .try_borrow()
            .ok()?
            .descriptions
            .get(key)?
            .upgrade()?;
        self.publish_heap_identity(GraphIdentity::InterfaceDescription(description.scan_id))
            .ok()?;
        Some(description)
    }

    /// Called after complete preparation and before publication, without a safepoint.
    pub(crate) fn cache_interface_description(
        &self,
        key: InterfaceDescriptionKey,
        description: &Arc<InterfaceDescription>,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.publish_heap_identity(GraphIdentity::InterfaceDescription(description.scan_id))?;
        let mut store = self
            .interfaces
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("interface snapshot store is borrowed"))?;
        store
            .descriptions
            .try_reserve(1)
            .map_err(|_| self.resource_limit("interface description registry"))?;
        store.descriptions.insert(key, Arc::downgrade(description));
        Ok(())
    }

    pub(crate) fn alloc_interface_snapshot(
        &self,
        interface: InterfaceValueSnapshot,
    ) -> Result<InterfaceSnapshotId, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.publish_value(interface.data)?;
        self.publish_heap_identity(GraphIdentity::InterfaceDescription(
            interface.description.scan_id,
        ))?;
        for parent in &interface.parent_views {
            if let Some(parent) = parent.get() {
                self.publish_heap_identity(GraphIdentity::Interface(*parent))?;
            }
        }
        let mut graph = self
            .graph
            .try_lock()
            .map_err(|_| RuntimeError::module_validation("graph admission is borrowed"))?;
        graph.prepare_identity()?;
        let id = self
            .interfaces
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("interface snapshot store is borrowed"))?
            .insert(interface)
            .map_err(|_| self.resource_limit("interface snapshot storage"))?;
        self.with_heap_mark(GraphIdentity::Interface(id), |mark| {
            graph.admit_prepared(GraphIdentity::Interface(id), mark);
            Ok(())
        })?;
        Ok(id)
    }

    pub(crate) fn interface_metadata(
        &self,
        id: InterfaceSnapshotId,
    ) -> Option<Ref<'_, InterfaceValueSnapshot>> {
        if !self.heap_identity_accessible(GraphIdentity::Interface(id)) {
            return None;
        }
        Ref::filter_map(self.interfaces.try_borrow().ok()?, |store| store.get(id)).ok()
    }
}

#[cfg(test)]
mod tests;
