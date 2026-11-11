//! One complete source; stable coordinates survive growth and execution reuse.
use super::{ScopedValue, StoredValue};
use kagari_gc::{
    access::Scope,
    error::GcError,
    root_source::{RootAccess, RootSource},
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ExecutionId {
    owner: u64,
    index: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Bank {
    Managed,
    Local,
    Dependency,
    Pending,
}

impl Bank {
    fn index(self) -> usize {
        match self {
            Self::Managed => 0,
            Self::Local => 1,
            Self::Dependency => 2,
            Self::Pending => 3,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Location {
    pub execution: ExecutionId,
    pub bank: Bank,
    pub slot: usize,
}

pub(super) struct ExecutionStorage {
    banks: [Vec<StoredValue>; 4],
    pub scalars: Vec<u64>,
    pub initialized: Vec<bool>,
    pub logical_calls: Vec<usize>,
}

impl ExecutionStorage {
    pub fn backing_bytes(&self) -> usize {
        self.banks
            .iter()
            .map(|bank| bank.capacity() * size_of::<StoredValue>())
            .sum::<usize>()
            + self.scalars.capacity() * size_of::<u64>()
            + self.initialized.capacity() * size_of::<bool>()
            + self.logical_calls.capacity() * size_of::<usize>()
    }
}

struct Entry {
    generation: u64,
    storage: Option<ExecutionStorage>,
}

pub(super) struct ExecutionRoots {
    owner: u64,
    entries: Vec<Entry>,
}

pub(super) struct Cursor {
    end: usize,
    execution: usize,
    active: Option<ExecutionId>,
    bank: usize,
    slot: usize,
    frontier: [usize; 4],
}

pub(super) struct Slots<'a, 'gc> {
    source: &'a mut ExecutionRoots,
    cx: &'a mut Scope<'gc>,
}

impl ExecutionRoots {
    pub fn with<'gc, T>(
        roots: &mut RootAccess<'_, Self>,
        cx: &mut Scope<'gc>,
        operation: impl FnOnce(&mut Slots<'_, 'gc>) -> Result<T, GcError>,
    ) -> Result<T, GcError> {
        // SAFETY: the paired source remains in its driver. Only this checked
        // facade is lent; publication precedes every managed commit.
        let source = unsafe { roots.source_mut(cx)? };
        operation(&mut Slots { source, cx })
    }
}

impl<'gc> Slots<'_, 'gc> {
    pub fn enter(&mut self, slots: usize) -> Result<ExecutionId, GcError> {
        let mut managed = Vec::new();
        managed.try_reserve_exact(slots)?;
        managed.resize_with(slots, || StoredValue::Unit);
        let storage = ExecutionStorage {
            banks: [managed, Vec::new(), Vec::new(), vec![StoredValue::Unit]],
            scalars: Vec::new(),
            initialized: Vec::new(),
            logical_calls: Vec::new(),
        };
        let index = self
            .source
            .entries
            .iter()
            .position(|entry| entry.storage.is_none() && entry.generation != u64::MAX);
        let index = match index {
            Some(index) => {
                self.source.entries[index].generation += 1;
                self.source.entries[index].storage = Some(storage);
                index
            }
            None => {
                self.source.entries.try_reserve(1)?;
                self.source.entries.push(Entry {
                    generation: 1,
                    storage: Some(storage),
                });
                self.source.entries.len() - 1
            }
        };
        Ok(ExecutionId {
            owner: self.source.owner,
            index,
            generation: self.source.entries[index].generation,
        })
    }

    pub fn storage(&mut self, id: ExecutionId) -> Result<&mut ExecutionStorage, GcError> {
        if self.source.owner != id.owner {
            return Err(GcError::ForeignHeap);
        }
        self.source
            .entries
            .get_mut(id.index)
            .filter(|entry| entry.generation == id.generation)
            .and_then(|entry| entry.storage.as_mut())
            .ok_or(GcError::Index)
    }

    pub fn leave(&mut self, id: ExecutionId) -> Result<(), GcError> {
        self.storage(id)?;
        self.source.entries[id.index].storage = None;
        Ok(())
    }

    pub fn grow(&mut self, id: ExecutionId, bank: Bank, count: usize) -> Result<(), GcError> {
        let values = &mut self.storage(id)?.banks[bank.index()];
        values.try_reserve(count.saturating_sub(values.len()))?;
        if count > values.len() {
            values.resize_with(count, || StoredValue::Unit);
        }
        Ok(())
    }

    pub fn truncate(&mut self, id: ExecutionId, bank: Bank, count: usize) -> Result<(), GcError> {
        self.storage(id)?.banks[bank.index()].truncate(count);
        Ok(())
    }

    pub fn len(&mut self, id: ExecutionId, bank: Bank) -> Result<usize, GcError> {
        Ok(self.storage(id)?.banks[bank.index()].len())
    }

    pub fn get(&mut self, location: Location) -> Result<ScopedValue<'gc>, GcError> {
        self.storage(location.execution)?;
        let value = self.source.entries[location.execution.index]
            .storage
            .as_ref()
            .and_then(|storage| storage.banks[location.bank.index()].get(location.slot))
            .ok_or(GcError::Index)?;
        Ok(match value {
            StoredValue::Unit => ScopedValue::Unit,
            StoredValue::I64(value) => ScopedValue::I64(*value),
            // SAFETY: the admitted location retains this exact same-driver edge;
            // no collection or mutation occurs before constructing its handle.
            StoredValue::Managed(edge) => {
                ScopedValue::Managed(unsafe { self.cx.source_get(edge)? })
            }
        })
    }

    pub fn set(&mut self, location: Location, value: ScopedValue<'gc>) -> Result<(), GcError> {
        self.storage(location.execution)?.banks[location.bank.index()]
            .get(location.slot)
            .ok_or(GcError::Index)?;
        let stored = match value {
            ScopedValue::Unit => StoredValue::Unit,
            ScopedValue::I64(value) => StoredValue::I64(value),
            // SAFETY: publish before an infallible coupled slot commit into the
            // paired source. No carrier or unchecked tag can escape this facade.
            ScopedValue::Managed(value) => {
                StoredValue::Managed(unsafe { self.cx.source_edge(value)? })
            }
        };
        self.storage(location.execution)?.banks[location.bank.index()][location.slot] = stored;
        Ok(())
    }

    pub fn copy(&mut self, destination: Location, source: Location) -> Result<(), GcError> {
        let value = self.get(source)?;
        self.set(destination, value)
    }

    pub fn scope(&mut self) -> &mut Scope<'gc> {
        self.cx
    }
}

// SAFETY: source mutation is encapsulated by Slots and publishes all incoming
// edges. Slots are born empty. Captured finite frontiers use coordinates and
// generations, never Vec addresses; changed/new suffixes publish independently.
// Every bank (including locals, dependencies and pending) is scanned. Removing a
// slot/execution drops inert carriers without dereferencing any managed target.
unsafe impl RootSource for ExecutionRoots {
    type Cursor = Cursor;

    fn empty() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            owner: NEXT
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("proof source identities"),
            entries: Vec::new(),
        }
    }

    fn begin_scan(&self) -> Result<Cursor, GcError> {
        Ok(Cursor {
            end: self.entries.len(),
            execution: 0,
            active: None,
            bank: 0,
            slot: 0,
            frontier: [0; 4],
        })
    }

    fn scan(
        &self,
        cursor: &mut Cursor,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while cursor.execution < cursor.end {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let entry = &self.entries[cursor.execution];
            let id = ExecutionId {
                owner: self.owner,
                index: cursor.execution,
                generation: entry.generation,
            };
            if let Some(storage) = &entry.storage
                && cursor.active.is_none_or(|active| active == id)
            {
                if cursor.active.is_none() {
                    cursor.active = Some(id);
                    cursor.frontier = std::array::from_fn(|bank| storage.banks[bank].len());
                    cursor.bank = 0;
                    cursor.slot = 0;
                }
                while cursor.bank < 4 {
                    while cursor.slot < cursor.frontier[cursor.bank] {
                        if !budget.take(1) {
                            return Ok(ScanStatus::Pending);
                        }
                        if let Some(StoredValue::Managed(edge)) = storage.banks[cursor.bank].get(cursor.slot)
                            // SAFETY: retained, initialized slot of this paired
                            // source. Pending retries before advancing coverage.
                            && unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending
                        {
                            return Ok(ScanStatus::Pending);
                        }
                        cursor.slot += 1;
                    }
                    cursor.bank += 1;
                    cursor.slot = 0;
                }
            }
            cursor.execution += 1;
            cursor.active = None;
        }
        Ok(ScanStatus::Complete)
    }
}
