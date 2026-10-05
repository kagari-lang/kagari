//! Heap-owned roots. Leases retain entries without owning their values or storage.
use crate::{gc::GcHeap, value::Value};
use std::sync::{Arc, Weak};

#[derive(Debug)]
struct RootLease;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RootId {
    owner: u64,
    slot: usize,
    generation: u64,
}

/// Host retention. Clones share a lease; values are accessed through the owning heap.
#[must_use = "retain this handle for as long as the host needs the value"]
#[derive(Debug, Clone)]
pub struct RootedValue {
    roots: RootSet,
}

impl RootedValue {
    /// Read a protected value after checking the heap identity and root generation.
    pub fn value(&self, heap: &GcHeap) -> Option<Value> {
        self.roots.get(heap, 0)
    }

    pub fn set(&self, heap: &GcHeap, value: Value) -> Option<()> {
        if !value.is_storable() {
            return None;
        }
        self.roots.set(heap, 0, value)
    }
}

/// A lease on execution slots stored in the heap's root table.
#[must_use = "retain the registered slots until execution resources are released"]
#[derive(Debug, Clone)]
pub struct RootSet {
    id: RootId,
    lease: Arc<RootLease>,
}

impl PartialEq for RootSet {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Arc::ptr_eq(&self.lease, &other.lease)
    }
}

impl RootSet {
    pub(crate) fn contains_slot(&self, heap: &GcHeap, index: usize) -> bool {
        self.with_value(heap, index, |_| ()).is_some()
    }

    pub fn get(&self, heap: &GcHeap, index: usize) -> Option<Value> {
        self.with_value(heap, index, Value::clone)
    }

    pub(crate) fn with_value<R>(
        &self,
        heap: &GcHeap,
        index: usize,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        if self.id.owner != heap.owner {
            return None;
        }
        let roots = heap.roots.try_borrow().ok()?;
        roots.entry(self)?.values.get(index).map(read)
    }

    pub fn set(&self, heap: &GcHeap, index: usize, value: Value) -> Option<()> {
        if self.id.owner != heap.owner || !heap.validate_value(&value) {
            return None;
        }
        let mut roots = heap.roots.try_borrow_mut().ok()?;
        roots.entry(self)?;
        *roots
            .slots
            .get_mut(self.id.slot)?
            .entry
            .as_mut()?
            .values
            .get_mut(index)? = value;
        Some(())
    }
}

#[derive(Debug)]
struct RootEntry {
    lease: Weak<RootLease>,
    values: Vec<Value>,
}

#[derive(Debug)]
struct RootSlot {
    generation: u64,
    entry: Option<RootEntry>,
}

#[derive(Debug, Default)]
pub(super) struct RootTable {
    slots: Vec<RootSlot>,
    free: Vec<usize>,
}

impl RootTable {
    fn entry(&self, root: &RootSet) -> Option<&RootEntry> {
        let slot = self.slots.get(root.id.slot)?;
        let entry = slot.entry.as_ref()?;
        (slot.generation == root.id.generation && entry.lease.ptr_eq(&Arc::downgrade(&root.lease)))
            .then_some(entry)
    }

    fn insert(&mut self, owner: u64, values: Vec<Value>) -> RootSet {
        self.prune();
        let index = self.free.pop().unwrap_or_else(|| {
            self.slots.push(RootSlot {
                generation: 0,
                entry: None,
            });
            self.slots.len() - 1
        });
        let slot = &mut self.slots[index];
        let lease = Arc::new(RootLease);
        slot.entry = Some(RootEntry {
            lease: Arc::downgrade(&lease),
            values,
        });
        RootSet {
            id: RootId {
                owner,
                slot: index,
                generation: slot.generation,
            },
            lease,
        }
    }

    pub(super) fn prune(&mut self) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot
                .entry
                .as_ref()
                .is_some_and(|entry| entry.lease.strong_count() == 0)
            {
                slot.entry = None;
                if let Some(generation) = slot.generation.checked_add(1) {
                    slot.generation = generation;
                    self.free.push(index);
                }
            }
        }
    }

    pub(super) fn active(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| {
                slot.entry
                    .as_ref()
                    .is_some_and(|entry| entry.lease.strong_count() > 0)
            })
            .count()
    }

    /// Hold each live lease while copying its roots. A concurrent last drop can
    /// conservatively retain an object for this collection, never remove a live root.
    fn snapshots(&mut self) -> Vec<Value> {
        self.prune();
        let mut values = Vec::new();
        for slot in &self.slots {
            if let Some(entry) = &slot.entry
                && let Some(_lease) = entry.lease.upgrade()
            {
                values.extend_from_slice(&entry.values);
            }
        }
        values
    }
}

impl GcHeap {
    pub fn root_value(&self, value: Value) -> Option<RootedValue> {
        if !value.is_storable() {
            return None;
        }
        Some(RootedValue {
            roots: self.root_execution_values(vec![value])?,
        })
    }

    pub fn root_execution_values(&self, values: Vec<Value>) -> Option<RootSet> {
        self.ensure_execution_allowed().ok()?;
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        Some(self.roots.try_borrow_mut().ok()?.insert(self.owner, values))
    }

    pub(super) fn root_snapshots(&self) -> Option<Vec<Value>> {
        Some(self.roots.try_borrow_mut().ok()?.snapshots())
    }
}

#[cfg(test)]
mod tests;
