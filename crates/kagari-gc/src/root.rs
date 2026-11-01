//! Owning root leases, fixed-cardinality packs and allocation-free release notices.

use crate::{
    error::GcError,
    object::ObjectRef,
    table::{Table, shrink_target, shrink_vec},
};
use std::{
    any::TypeId,
    collections::VecDeque,
    fmt,
    marker::PhantomData,
    ptr,
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

/// An owning handle. Cloning shares its registration; dropping never runs GC.
///
/// Handles can outlive heap shutdown as inert tokens. Cross-heap operations fail
/// before reading a stored object address. No object data is accessible directly.
pub struct Root<T: 'static> {
    pub(crate) lease: Arc<RootLease>,
    pub(crate) marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for Root<T> {
    fn clone(&self) -> Self {
        Self {
            lease: self.lease.clone(),
            marker: PhantomData,
        }
    }
}

impl<T: 'static> fmt::Debug for Root<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Root").field(&self.lease.id).finish()
    }
}

/// A fixed number of replaceable root slots sharing one lifetime registration.
#[derive(Clone)]
pub struct RootSet {
    pub(crate) lease: Arc<RootLease>,
    pub(crate) len: usize,
}

impl RootSet {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl fmt::Debug for RootSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RootSet")
            .field("id", &self.lease.id)
            .field("len", &self.len)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RootId {
    pub(crate) heap: u64,
    pub(crate) slot: u32,
    pub(crate) generation: u64,
}

#[derive(Debug)]
pub(crate) struct RootLease {
    pub(crate) id: RootId,
    released: Arc<Mutex<ReleasePort>>,
    armed: AtomicBool,
}

impl Drop for RootLease {
    fn drop(&mut self) {
        if !self.armed.load(Ordering::Relaxed) {
            return;
        }
        let mut port = lock(&self.released);
        if port.connected {
            assert!(
                port.pending.len() < port.pending.capacity(),
                "reserved root-release credit"
            );
            port.pending.push_back(self.id);
        }
    }
}

#[derive(Debug)]
struct ReleasePort {
    connected: bool,
    credits: usize,
    pending: VecDeque<RootId>,
}

#[derive(Debug)]
pub(crate) struct RootEntry {
    active_index: usize,
    pub(crate) lease: Weak<RootLease>,
    pub(crate) targets: Box<[Option<ObjectRef>]>,
    pub(crate) typed: Option<TypeId>,
}

#[derive(Debug)]
pub(crate) struct RootSlot {
    pub(crate) generation: u64,
    pub(crate) entry: RootEntry,
}

#[derive(Debug)]
pub(crate) struct Roots {
    heap: u64,
    pub(crate) slots: Table<RootSlot>,
    pub(crate) active: Vec<u32>,
    serial: u64,
    released: Arc<Mutex<ReleasePort>>,
    pub(crate) count: usize,
}

pub(crate) struct PreparedRoot {
    pub(crate) lease: Arc<RootLease>,
    entry: RootEntry,
}

impl Roots {
    pub(crate) fn new(heap: u64) -> Self {
        Self {
            heap,
            slots: Table::default(),
            active: Vec::new(),
            serial: 0,
            count: 0,
            released: Arc::new(Mutex::new(ReleasePort {
                connected: true,
                credits: 0,
                pending: VecDeque::new(),
            })),
        }
    }

    pub(crate) fn prepare(
        &mut self,
        count: usize,
        typed: Option<TypeId>,
        limit: usize,
    ) -> Result<PreparedRoot, GcError> {
        if self.count >= limit {
            return Err(GcError::Limit("root registrations"));
        }
        self.active.try_reserve(1)?;
        let mut targets = Vec::new();
        targets.try_reserve_exact(count)?;
        targets.resize(count, None);
        let slot = self.slots.prepare()?;
        let generation = self
            .serial
            .checked_add(1)
            .ok_or(GcError::Limit("root generation"))?;
        self.serial = generation;
        let mut port = lock(&self.released);
        let capacity = port
            .credits
            .checked_add(1)
            .ok_or(GcError::Limit("release credits"))?;
        let extra = capacity.saturating_sub(port.pending.len());
        port.pending.try_reserve(extra)?;
        drop(port);
        let lease = Arc::new(RootLease {
            id: RootId {
                heap: self.heap,
                slot,
                generation,
            },
            released: self.released.clone(),
            armed: AtomicBool::new(false),
        });
        Ok(PreparedRoot {
            entry: RootEntry {
                active_index: self.active.len(),
                lease: Arc::downgrade(&lease),
                targets: targets.into_boxed_slice(),
                typed,
            },
            lease,
        })
    }

    pub(crate) fn commit(
        &mut self,
        mut prepared: PreparedRoot,
        target: Option<ObjectRef>,
    ) -> Arc<RootLease> {
        if let Some(first) = prepared.entry.targets.first_mut() {
            *first = target;
        }
        let id = prepared.lease.id;
        self.active.push(id.slot);
        self.slots.insert(
            id.slot,
            RootSlot {
                generation: id.generation,
                entry: prepared.entry,
            },
        );
        self.count += 1;
        lock(&self.released).credits += 1;
        prepared.lease.armed.store(true, Ordering::Relaxed);
        prepared.lease
    }

    pub(crate) fn admit(&self, lease: &Arc<RootLease>) -> Result<&RootEntry, GcError> {
        let id = lease.id;
        if id.heap != self.heap {
            return Err(GcError::ForeignHeap);
        }
        let slot = self
            .slots
            .get(id.slot)
            .filter(|slot| slot.generation == id.generation)
            .ok_or(GcError::InvalidRoot)?;
        let entry = &slot.entry;
        if !ptr::eq(entry.lease.as_ptr(), Arc::as_ptr(lease)) {
            return Err(GcError::InvalidRoot);
        }
        Ok(entry)
    }

    pub(crate) fn target<T: 'static>(&self, root: &Root<T>) -> Result<ObjectRef, GcError> {
        let entry = self.admit(&root.lease)?;
        if entry.typed != Some(TypeId::of::<T>()) {
            return Err(GcError::Representation);
        }
        entry.targets[0].ok_or(GcError::InvalidRoot)
    }

    /// Admit once, prepare/publicize a target through shared root access, then
    /// commit while the exclusive table loan prevents invalidation of the slot.
    pub(crate) fn replace_with(
        &mut self,
        roots: &RootSet,
        index: usize,
        prepare: impl FnOnce(&Self) -> Result<Option<ObjectRef>, GcError>,
    ) -> Result<(), GcError> {
        let entry = self.admit(&roots.lease)?;
        if entry.typed.is_some() || index >= entry.targets.len() {
            return Err(GcError::Index);
        }
        let target = prepare(self)?;
        self.slots[roots.lease.id.slot].entry.targets[index] = target;
        Ok(())
    }

    /// Release a single finite notification; pack destruction occurs outside loans.
    pub(crate) fn release_one(&mut self) -> Option<RootEntry> {
        let id = lock(&self.released).pending.pop_front()?;
        let slot = self.slots.remove(id.slot).expect("registered release slot");
        assert_eq!(slot.generation, id.generation);
        let entry = slot.entry;
        assert_eq!(entry.lease.strong_count(), 0);
        self.active.swap_remove(entry.active_index);
        if let Some(&moved) = self.active.get(entry.active_index) {
            self.slots[moved].entry.active_index = entry.active_index;
        }
        self.count -= 1;
        lock(&self.released).credits -= 1;
        Some(entry)
    }

    pub(crate) fn disconnect(&mut self) {
        let mut port = lock(&self.released);
        port.connected = false;
        port.pending = VecDeque::new();
        port.credits = 0;
    }

    pub(crate) fn close(&mut self) {
        self.slots = Table::default();
        self.active = Vec::new();
        self.count = 0;
    }

    pub(crate) fn pending_releases(&self) -> usize {
        lock(&self.released).pending.len()
    }

    pub(crate) fn trim(&mut self) {
        self.slots.trim();
        shrink_vec(&mut self.active, self.count);
        let mut port = lock(&self.released);
        // Every outstanding lease, including an already queued release, owns
        // one credit. Concurrent final drops cannot race this capacity change.
        if let Some(target) = shrink_target(port.pending.capacity(), port.credits) {
            port.pending.shrink_to(target);
        }
    }

    pub(crate) fn buffer_storage(&self) -> (usize, usize) {
        (
            self.active.capacity() * size_of::<u32>(),
            lock(&self.released).pending.capacity() * size_of::<RootId>(),
        )
    }
}

fn lock(port: &Mutex<ReleasePort>) -> MutexGuard<'_, ReleasePort> {
    port.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_survive_block_recreation_and_reject_exhaustion() -> Result<(), GcError> {
        let mut roots = Roots::new(1);
        let prepared = roots.prepare(0, None, usize::MAX)?;
        let lease = roots.commit(prepared, None);
        let first = lease.id;
        drop(lease);
        roots.release_one().unwrap();
        assert_eq!(roots.slots.storage(), (0, 0));
        roots.serial = u64::MAX - 1;
        let prepared = roots.prepare(0, None, usize::MAX)?;
        let last = roots.commit(prepared, None);
        assert_eq!(last.id.slot, first.slot);
        assert!(last.id.generation > first.generation);
        assert!(matches!(
            roots.prepare(0, None, usize::MAX),
            Err(GcError::Limit("root generation"))
        ));
        assert!(roots.admit(&last).is_ok());
        drop(last);
        roots.release_one().unwrap();
        assert!(matches!(
            roots.prepare(0, None, usize::MAX),
            Err(GcError::Limit("root generation"))
        ));
        roots.trim();
        assert_eq!(roots.slots.storage(), (0, 0));
        Ok(())
    }
}
