//! Heap-owned roots. Leases retain entries without owning their values or storage.
pub(crate) mod scanning;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    gc::{
        GcHeap,
        metrics::{IndivisibleKind, IndivisibleSample},
    },
    native::{
        binding::NativeResult,
        storage::scan::{ScanBudget, ScanStatus},
    },
    value::Value,
};
use std::{
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
};

#[derive(Debug)]
struct RootLease {
    id: RootId,
    released: Arc<Mutex<RootReleases>>,
}

impl Drop for RootLease {
    fn drop(&mut self) {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::RootRelease);
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
    pub(crate) fn value(&self, heap: &GcHeap) -> Option<Value> {
        self.roots.get(heap, 0)
    }

    /// Replace this shared root from another live root in the same heap.
    /// Clones observe replacement; the target lease and its metadata stay shared.
    /// Foreign/expired sources and targets, invalid values and excluded mutation reject.
    pub fn replace(&self, heap: &GcHeap, source: &Self) -> Option<()> {
        self.set(heap, source.value(heap)?)
    }

    /// Read backend transport with caller-owned lifetime protection.
    ///
    /// # Safety
    /// Keep the referent protected until every copy is consumed or published to
    /// another traced owner. Cloning this mutable root alone does not protect an
    /// old copy against replacement through an alias. Terminal backend handoffs
    /// must not collect between releasing this lease and destination publication.
    /// Detached copies of direct references cannot be used for expiry probes.
    /// Check the root token or retain a live wrong-kind/foreign input instead;
    /// validation cannot safely inspect an already reclaimed address.
    #[doc(hidden)]
    pub unsafe fn value_transport(&self, heap: &GcHeap) -> Option<Value> {
        self.value(heap)
    }

    /// Publish already protected backend transport into this root.
    ///
    /// # Safety
    /// The new value must remain live in its originating heap through publication.
    /// In-flight raw readers of this root must have independent protection before
    /// replacement can release their source. Stale transport is not a valid input.
    #[doc(hidden)]
    pub unsafe fn set_transport(&self, heap: &GcHeap, value: Value) -> Option<()> {
        self.set(heap, value)
    }

    pub(crate) fn set(&self, heap: &GcHeap, value: Value) -> Option<()> {
        if !value.is_storable(heap) {
            return None;
        }
        self.roots.set(heap, 0, value)
    }
}

/// Immutable host retention, with no root-table entry for edge-free scalar data.
/// Supplied mutable roots are never demoted: their aliases and metadata stay shared.
#[derive(Debug, Clone)]
pub(crate) struct RetainedValue {
    retention: Retention,
}

#[derive(Debug, Clone)]
enum Retention {
    Scalar { owner: u64, value: Value },
    Rooted(RootedValue),
}

impl RetainedValue {
    pub(crate) fn from_root(root: RootedValue) -> Self {
        Self {
            retention: Retention::Rooted(root),
        }
    }

    pub(crate) fn as_root(&self) -> Option<&RootedValue> {
        match &self.retention {
            Retention::Scalar { .. } => None,
            Retention::Rooted(root) => Some(root),
        }
    }

    pub(crate) fn value(&self, heap: &GcHeap) -> Option<Value> {
        match &self.retention {
            Retention::Scalar { owner, value } => {
                if *owner != heap.owner {
                    return None;
                }
                // Preserve the root owner's read exclusion without renting a slot.
                let _roots = heap.roots.try_borrow().ok()?;
                Some(*value)
            }
            Retention::Rooted(root) => root.value(heap),
        }
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
            runtime.publish_metadata_edge(root.edge())?;
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

    pub(crate) fn slot_count(&self, heap: &GcHeap) -> Option<usize> {
        if !self.belongs_to(heap) {
            return None;
        }
        Some(heap.roots.try_borrow().ok()?.entry(self)?.values.len())
    }

    pub(crate) fn get(&self, heap: &GcHeap, index: usize) -> Option<Value> {
        self.with_value(heap, index, Value::clone)
    }

    /// Replace one slot from a live, storable root in the same heap.
    /// Invalid owners, generations, bounds and excluded mutation reject.
    pub fn replace(&self, heap: &GcHeap, index: usize, source: &RootedValue) -> Option<()> {
        self.set(heap, index, source.value(heap)?)
    }

    /// Read transport from an execution root pack.
    ///
    /// # Safety
    /// Keep the selected value live until every copy is consumed or published.
    /// A pack alias can replace the slot, so a shared lease alone is insufficient
    /// across arbitrary callbacks. Scoped host references also require their
    /// original host scope, which this pack does not extend.
    #[doc(hidden)]
    pub unsafe fn get_transport(&self, heap: &GcHeap, index: usize) -> Option<Value> {
        self.get(heap, index)
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

    pub(crate) fn set(&self, heap: &GcHeap, index: usize, value: Value) -> Option<()> {
        heap.ensure_execution_allowed().ok()?;
        if self.id.owner != heap.owner {
            return None;
        }
        heap.checked_target(value)?.publish().ok()?;
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
    #[cfg(test)]
    disposal_probe: Option<tests::DisposalProbe>,
}

impl RootEntry {
    /// Each top-level dependency gets its own unwind boundary, so one panic does
    /// not cause remaining dependencies to be dropped during that unwind.
    fn dispose(self) -> bool {
        drop(self.lease);
        drop(self.values);
        let mut panicked = false;
        #[cfg(test)]
        {
            panicked |= catch_unwind(AssertUnwindSafe(|| drop(self.disposal_probe))).is_err();
        }
        for dependency in self.metadata {
            panicked |= catch_unwind(AssertUnwindSafe(|| drop(dependency))).is_err();
        }
        panicked
    }
}

#[derive(Debug)]
struct RootSlot {
    generation: u64,
    entry: Option<RootEntry>,
}

/// A finite release allowance captured from the notification stack.
/// Later releases can replace pending work, but cannot extend this drain.
#[derive(Debug)]
pub(crate) struct RootReleaseCursor {
    remaining: usize,
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

    fn insert(&mut self, values: Vec<Value>) -> (Option<RootSet>, Option<RootEntry>) {
        // Detach at most one released slot. The caller must dispose its dependencies
        // after releasing the table borrow, even if admission fails.
        let (_, retired) = self.drain_released_limit(1);
        (self.register(values), retired)
    }

    fn register(&mut self, values: Vec<Value>) -> Option<RootSet> {
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
            #[cfg(test)]
            disposal_probe: None,
        });
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::RootRegistration);
        Some(RootSet { id, lease })
    }

    pub(super) fn begin_release_drain(&self) -> RootReleaseCursor {
        RootReleaseCursor {
            remaining: lock_releases(&self.released).pending.len(),
        }
    }

    /// Detach at most one record; the heap owner disposes it outside this borrow.
    /// Every inspection, including stale notifications and empty probes, is charged.
    fn drain_release_slice(
        &mut self,
        cursor: &mut RootReleaseCursor,
        budget: &mut ScanBudget,
    ) -> (ScanStatus, Option<RootEntry>) {
        while cursor.remaining != 0 {
            if !budget.take(1) {
                return (ScanStatus::Pending, None);
            }
            let (processed, retired) = self.drain_released_limit(1);
            if processed == 0 {
                cursor.remaining = 0;
            } else {
                cursor.remaining -= 1;
            }
            if retired.is_some() {
                let status = if cursor.remaining == 0 {
                    ScanStatus::Complete
                } else {
                    ScanStatus::Pending
                };
                return (status, retired);
            }
        }
        (ScanStatus::Complete, None)
    }

    /// Inspect at most `limit` IDs, stopping at the first detachable record.
    /// Returns inspected IDs (including stale ones) and ownership for disposal
    /// outside the table borrow. No allocation or dependency destruction occurs here.
    fn drain_released_limit(&mut self, limit: usize) -> (usize, Option<RootEntry>) {
        let mut processed = 0;
        while processed < limit {
            let Some(id) = lock_releases(&self.released).pending.pop() else {
                break;
            };
            let entry = self.release(id);
            lock_releases(&self.released).credits -= 1;
            processed += 1;
            if entry.is_some() {
                return (processed, entry);
            }
        }
        (processed, None)
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

    #[cfg(test)]
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
    #[cfg(test)]
    pub(super) fn metadata_snapshots(&self) -> Option<Vec<MetadataRoot>> {
        let mut roots = self.roots.try_borrow_mut().ok()?.metadata_snapshots();
        self.resources
            .frame_values
            .try_borrow()
            .ok()?
            .append_metadata(&mut roots);
        Some(roots)
    }

    /// Publish live backend transport into an independently owned script root.
    /// Nonstorable values and failed runtime/publication admission are rejected.
    ///
    /// # Safety
    /// Keep the input live through publication, using its existing traced owner
    /// or immediate allocation/result handoff with no intervening collection.
    /// Rooting cannot revive an expired reference. Source aliases must not replace
    /// the input before publication. Ordinary hosts use typed conversion or scoped
    /// retention instead of raw transport.
    /// The checked-ID implementation also permits non-dereferencing invalid-ID
    /// rejection probes. They are not live backend inputs and must be replaced
    /// with inert identity/retained-wrapper checks before direct references.
    #[doc(hidden)]
    pub unsafe fn root_value_transport(&self, value: Value) -> Option<RootedValue> {
        self.root_value(value)
    }

    pub(crate) fn root_value(&self, value: Value) -> Option<RootedValue> {
        if !value.is_storable(self) {
            return None;
        }
        Some(RootedValue {
            roots: self.root_execution_values(vec![value])?,
        })
    }

    /// Retain an immutable value with ordinary execution and publication checks.
    pub(crate) fn retain_value(&self, value: Value) -> Option<RetainedValue> {
        if !value.is_storable(self) {
            return None;
        }
        self.ensure_execution_allowed().ok()?;
        self.checked_target(value)?.publish().ok()?;
        self.retain_published_value(value).ok()
    }

    /// Retain an existing storable value during an authorized native payload loan.
    /// StorageAccess owns declared-type and recovery checks; this checks execution,
    /// identity and root admission without lifting the loan's GC exclusion.
    pub(crate) fn retain_native_value(&self, value: Value) -> NativeResult<RetainedValue> {
        self.resources.ensure_execution_allowed()?;
        if self.storage_borrows.get() == 0 {
            return Err(RuntimeError::module_validation(
                "native root retention requires an active payload loan",
            ));
        }
        let invalid = || {
            RuntimeError::module_validation("invalid native root value or conflicting heap borrow")
        };
        if !value.is_storable(self) {
            return Err(invalid());
        }
        self.checked_target(value).ok_or_else(invalid)?.publish()?;
        self.retain_published_value(value)
    }

    fn retain_published_value(&self, value: Value) -> NativeResult<RetainedValue> {
        if matches!(
            value,
            Value::Unit
                | Value::Bool(_)
                | Value::Char(_)
                | Value::I32(_)
                | Value::I64(_)
                | Value::U64(_)
                | Value::F32(_)
                | Value::F64(_)
        ) {
            // These variants contain neither heap IDs nor metadata. The checked,
            // never-reused heap owner is sufficient for an immutable scalar;
            // mutable roots still require their slot generation and shared lease.
            let _roots = self
                .roots
                .try_borrow_mut()
                .map_err(|_| RuntimeError::module_validation("root storage is borrowed"))?;
            return Ok(RetainedValue {
                retention: Retention::Scalar {
                    owner: self.owner,
                    value,
                },
            });
        }
        let mut values = Vec::new();
        values
            .try_reserve(1)
            .map_err(|_| self.resource_limit("native root value storage"))?;
        values.push(value);
        let roots = self.insert_root(values)?;
        Ok(RetainedValue::from_root(RootedValue { roots }))
    }

    /// Publish a complete live backend argument/snapshot pack into execution roots.
    /// Unlike an owning script value, these slots may retain scoped capabilities.
    ///
    /// # Safety
    /// Protect every input until the complete pack is published, including across
    /// earlier input preparation. Host capabilities require their original frame;
    /// an execution root does not extend their Rust ownership or scope. Detached
    /// expired references cannot be made valid by registering roots.
    ///
    /// ```compile_fail,E0133
    /// use kagari_runtime::{gc::GcHeap, value::Value};
    /// fn unprotected(heap: &GcHeap) {
    ///     let roots = heap.root_execution_values_transport(vec![Value::Unit]);
    /// }
    /// ```
    /// The checked-ID implementation also permits non-dereferencing invalid-ID
    /// rejection probes. They are not live backend inputs and must be replaced
    /// with inert identity/retained-wrapper checks before direct references.
    #[doc(hidden)]
    pub unsafe fn root_execution_values_transport(&self, values: Vec<Value>) -> Option<RootSet> {
        self.root_execution_values(values)
    }

    pub(crate) fn root_execution_values(&self, values: Vec<Value>) -> Option<RootSet> {
        self.ensure_execution_allowed().ok()?;
        self.publish_root_values(&values)?;
        self.insert_root(values).ok()
    }

    fn insert_root(&self, values: Vec<Value>) -> Result<RootSet, RuntimeError> {
        let (root, retired) = self
            .roots
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("root storage is borrowed"))?
            .insert(values);
        if let Some(retired) = retired {
            // Registration can run during ordinary execution or a native loan.
            // A final metadata owner must not reenter execution/collection here.
            self.resources.collect_operation(|| {
                self.dispose_root_entry(retired, &mut ScanBudget::new(0))
            })??;
        }
        root.ok_or_else(|| self.resource_limit("root registration"))
    }

    /// Service a finite allowance under the caller's collection exclusion. At most
    /// one root record is detached at a time, and its dependencies are destroyed
    /// outside the table/transport borrow before further inspection. Disposal may
    /// exceed the budget as indivisible work; no later record is inspected then.
    pub(super) fn drain_root_release_slice(
        &self,
        cursor: &mut RootReleaseCursor,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, RuntimeError> {
        loop {
            let (status, retired) = self
                .roots
                .try_borrow_mut()
                .map_err(|_| RuntimeError::module_validation("root storage is borrowed"))?
                .drain_release_slice(cursor, budget);
            let had_retired = retired.is_some();
            if let Some(retired) = retired {
                self.dispose_root_entry(retired, budget)?;
            }
            if status == ScanStatus::Complete || !had_retired {
                return Ok(status);
            }
        }
    }

    fn dispose_root_entry(
        &self,
        retired: RootEntry,
        budget: &mut ScanBudget,
    ) -> Result<(), RuntimeError> {
        // Account for every stored item plus release of the record backing. The
        // transitive cost of final Arc destruction is opaque and timed separately.
        let items = retired.values.len().saturating_add(retired.metadata.len());
        let work = items.saturating_add(1);
        budget.charge(work);
        let started = self.observation_clock().start();
        let panicked = retired.dispose();
        if let Some(started) = started {
            self.observe_indivisible(
                IndivisibleKind::RootDrop,
                IndivisibleSample::new(
                    "RootEntry",
                    items,
                    work,
                    Some(size_of::<RootEntry>()),
                    started.elapsed(),
                ),
            );
        }
        if panicked {
            return Err(self
                .resources
                .quarantine("root dependency destruction panicked"));
        }
        if self.resources.is_quarantined() {
            return Err(self
                .resources
                .quarantine("root dependency cleanup reentered execution"));
        }
        Ok(())
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
