//! Heap-owned roots. Leases retain entries without owning their values or storage.
use crate::{
    Runtime, error::RuntimeError, execution_metadata::MetadataRoot, gc::GcHeap,
    native::binding::NativeResult, value::Value,
};
use std::{
    mem,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
};

#[derive(Debug)]
struct RootLease {
    id: RootId,
    released: Arc<Mutex<RootReleases>>,
}

impl Drop for RootLease {
    fn drop(&mut self) {
        let mut released = lock_releases(&self.released);
        if released.connected {
            // Each registration reserves one credit until its final drop is processed.
            // Clones share this Drop, so pending.len() < capacity() before every push.
            released.pending.push(self.id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RootId {
    owner: u64,
    slot: usize,
    generation: u64,
}

/// ID-only transport; credits include live leases, queued IDs and an ID being drained.
#[derive(Debug)]
struct RootReleases {
    connected: bool,
    credits: usize,
    pending: Vec<RootId>,
}

impl RootReleases {
    fn reserve(&mut self) -> Option<()> {
        let credits = self.credits.checked_add(1)?;
        self.pending
            .try_reserve(credits.checked_sub(self.pending.len())?)
            .ok()?;
        self.credits = credits;
        Some(())
    }
}

fn lock_releases(released: &Mutex<RootReleases>) -> MutexGuard<'_, RootReleases> {
    // No user code runs under this lock. Poison must not make cleanup unwind or
    // discard notifications whose capacity was already reserved.
    released.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Host retention. Clones share a lease; values are accessed through the owning heap.
#[must_use = "retain this handle for as long as the host needs the value"]
#[derive(Debug, Clone)]
pub struct RootedValue {
    roots: RootSet,
}

impl RootedValue {
    pub(crate) fn is_valid(&self, heap: &GcHeap) -> bool {
        self.roots.contains_slot(heap, 0)
    }

    pub(crate) fn set_metadata(
        &self,
        runtime: &Runtime,
        metadata: Vec<MetadataRoot>,
    ) -> Result<(), RuntimeError> {
        self.roots.set_metadata(runtime, metadata)
    }

    /// Read a protected value after checking the heap identity and root generation.
    pub fn value(&self, heap: &GcHeap) -> Option<Value> {
        self.roots.get(heap, 0)
    }

    pub fn set(&self, heap: &GcHeap, value: Value) -> Option<()> {
        if !value.is_storable(heap) {
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
    pub(crate) fn set_metadata(
        &self,
        runtime: &Runtime,
        metadata: Vec<MetadataRoot>,
    ) -> Result<(), RuntimeError> {
        let heap = runtime.gc();
        heap.ensure_execution_allowed()?;
        if !self.belongs_to(heap) {
            return Err(RuntimeError::module_validation(
                "metadata root belongs to another runtime",
            ));
        }
        for root in &metadata {
            runtime.validate_metadata(root.edge())?;
        }
        let previous = {
            let mut roots = heap
                .roots
                .try_borrow_mut()
                .map_err(|_| RuntimeError::module_validation("root storage is borrowed"))?;
            roots
                .entry(self)
                .ok_or_else(|| RuntimeError::module_validation("invalid metadata root lease"))?;
            let entry = roots.slots[self.id.slot]
                .entry
                .as_mut()
                .expect("validated root lease");
            mem::replace(&mut entry.metadata, metadata)
        };
        drop(previous);
        Ok(())
    }

    /// Fault injection for collector atomicity tests, never a publication path.
    #[cfg(test)]
    pub(crate) fn corrupt_metadata_for_test(&self, heap: &GcHeap, metadata: Vec<MetadataRoot>) {
        let mut roots = heap.roots.borrow_mut();
        roots.entry(self).expect("valid fault-injection root");
        roots.slots[self.id.slot].entry.as_mut().unwrap().metadata = metadata;
    }

    pub(crate) fn belongs_to(&self, heap: &GcHeap) -> bool {
        self.id.owner == heap.owner
    }

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
        heap.ensure_execution_allowed().ok()?;
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
    metadata: Vec<MetadataRoot>,
}

#[derive(Debug)]
struct RootSlot {
    generation: u64,
    entry: Option<RootEntry>,
}

#[derive(Debug)]
pub(super) struct RootTable {
    owner: u64,
    slots: Vec<RootSlot>,
    free: Vec<usize>,
    released: Arc<Mutex<RootReleases>>,
    #[cfg(test)]
    release_visits: usize,
}

impl RootTable {
    pub(super) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            free: Vec::new(),
            released: Arc::new(Mutex::new(RootReleases {
                connected: true,
                credits: 0,
                pending: Vec::new(),
            })),
            #[cfg(test)]
            release_visits: 0,
        }
    }

    fn entry(&self, root: &RootSet) -> Option<&RootEntry> {
        if root.id.owner != self.owner {
            return None;
        }
        let slot = self.slots.get(root.id.slot)?;
        let entry = slot.entry.as_ref()?;
        (slot.generation == root.id.generation && entry.lease.ptr_eq(&Arc::downgrade(&root.lease)))
            .then_some(entry)
    }

    fn insert(&mut self, values: Vec<Value>) -> Option<RootSet> {
        self.drain_released();
        if self.free.is_empty() {
            self.slots.try_reserve(1).ok()?;
            // Every slot can enter the free list once, even if all leases drop together.
            let slots = self.slots.len().checked_add(1)?;
            self.free.try_reserve(slots - self.free.len()).ok()?;
        }
        lock_releases(&self.released).reserve()?;
        let index = self.free.pop().unwrap_or_else(|| {
            self.slots.push(RootSlot {
                generation: 0,
                entry: None,
            });
            self.slots.len() - 1
        });
        let slot = &mut self.slots[index];
        let id = RootId {
            owner: self.owner,
            slot: index,
            generation: slot.generation,
        };
        let lease = Arc::new(RootLease {
            id,
            released: self.released.clone(),
        });
        slot.entry = Some(RootEntry {
            lease: Arc::downgrade(&lease),
            values,
            metadata: Vec::new(),
        });
        Some(RootSet { id, lease })
    }

    pub(super) fn drain_released(&mut self) {
        self.drain_released_limit(usize::MAX);
    }

    /// Process at most `limit` release IDs, including rejected stale notifications.
    /// Returns inspected IDs, not retired slots. Transport capacity is retained.
    pub(super) fn drain_released_limit(&mut self, limit: usize) -> usize {
        let mut processed = 0;
        while processed < limit {
            let Some(id) = lock_releases(&self.released).pending.pop() else {
                break;
            };
            // Retire identity/credit before destroying detached contents, so even
            // a destructor panic cannot strand a slot or its notification credit.
            let entry = self.release(id);
            lock_releases(&self.released).credits -= 1;
            processed += 1;
            // Contents can release other leases; no transport lock spans their Drop.
            drop(entry);
        }
        processed
    }

    fn release(&mut self, id: RootId) -> Option<RootEntry> {
        #[cfg(test)]
        {
            self.release_visits += 1;
        }
        if id.owner != self.owner {
            return None;
        }
        let slot = self.slots.get_mut(id.slot)?;
        if slot.generation != id.generation
            || !slot
                .entry
                .as_ref()
                .is_some_and(|entry| entry.lease.strong_count() == 0)
        {
            return None;
        }
        let entry = slot.entry.take();
        if let Some(generation) = slot.generation.checked_add(1) {
            slot.generation = generation;
            self.free.push(id.slot);
        }
        entry
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
    fn visit_snapshots(&mut self, mut visit: impl FnMut(&RootEntry)) {
        self.drain_released();
        for slot in &self.slots {
            if let Some(entry) = &slot.entry
                && let Some(_lease) = entry.lease.upgrade()
            {
                visit(entry);
            }
        }
    }

    fn snapshots(&mut self) -> Vec<Value> {
        let mut values = Vec::new();
        self.visit_snapshots(|entry| values.extend_from_slice(&entry.values));
        values
    }

    fn metadata_snapshots(&mut self) -> Vec<MetadataRoot> {
        let mut metadata = Vec::new();
        self.visit_snapshots(|entry| metadata.extend_from_slice(&entry.metadata));
        metadata
    }
}

impl Drop for RootTable {
    fn drop(&mut self) {
        let mut released = lock_releases(&self.released);
        released.connected = false;
        released.credits = 0;
        // Late leases retain only the disconnected transport, not its high-water buffer.
        released.pending = Vec::new();
    }
}

impl Runtime {
    pub(crate) fn root_metadata(
        &self,
        metadata: Vec<MetadataRoot>,
    ) -> Result<RootSet, RuntimeError> {
        let roots = self
            .gc()
            .root_execution_values(Vec::new())
            .ok_or_else(|| RuntimeError::module_validation("metadata root allocation"))?;
        roots.set_metadata(self, metadata)?;
        Ok(roots)
    }
}

impl GcHeap {
    pub(super) fn metadata_snapshots(&self) -> Option<Vec<MetadataRoot>> {
        let mut roots = self.roots.try_borrow_mut().ok()?.metadata_snapshots();
        self.resources
            .frame_values
            .try_borrow()
            .ok()?
            .append_metadata(&mut roots);
        Some(roots)
    }

    pub fn root_value(&self, value: Value) -> Option<RootedValue> {
        if !value.is_storable(self) {
            return None;
        }
        Some(RootedValue {
            roots: self.root_execution_values(vec![value])?,
        })
    }

    /// Retain an existing storable edge during an authorized native payload loan.
    /// StorageAccess owns declared-type and recovery checks; this checks execution,
    /// identity and root-table admission without lifting the loan's GC exclusion.
    pub(crate) fn root_native_value(&self, value: Value) -> NativeResult<RootedValue> {
        self.resources.ensure_execution_allowed()?;
        if self.native_borrows.get() == 0 {
            return Err(RuntimeError::module_validation(
                "native root retention requires an active payload loan",
            ));
        }
        if !value.is_storable(self) || !self.validate_value(&value) {
            return Err(RuntimeError::module_validation(
                "invalid native root value or conflicting heap borrow",
            ));
        }
        let mut roots = self
            .roots
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("root storage is borrowed"))?;
        let mut values = Vec::new();
        values
            .try_reserve(1)
            .map_err(|_| self.resource_limit("native root value storage"))?;
        values.push(value);
        let roots = roots
            .insert(values)
            .ok_or_else(|| self.resource_limit("native root registration"))?;
        Ok(RootedValue { roots })
    }

    pub fn root_execution_values(&self, values: Vec<Value>) -> Option<RootSet> {
        self.ensure_execution_allowed().ok()?;
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        self.roots.try_borrow_mut().ok()?.insert(values)
    }

    pub(super) fn root_snapshots(&self) -> Option<Vec<Value>> {
        let mut roots = self.roots.try_borrow_mut().ok()?.snapshots();
        self.resources
            .frame_values
            .try_borrow()
            .ok()?
            .append_values(&mut roots);
        Some(roots)
    }
}

#[cfg(test)]
mod tests;
