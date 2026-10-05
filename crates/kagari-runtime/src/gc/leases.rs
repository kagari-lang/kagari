//! Heap-owned exclusion records. Guards never own storage or execution state.
use crate::{error::RuntimeError, gc::HeapObjectId};
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Weak},
};

/// Ending a session immediately expires all of its cursor leases.
#[derive(Debug, Default)]
pub(crate) struct LeaseScope(Arc<()>);

#[derive(Debug)]
struct LeaseLife {
    scope: Option<Weak<()>>,
}

impl LeaseLife {
    fn is_active(&self) -> bool {
        self.scope
            .as_ref()
            .is_none_or(|scope| scope.strong_count() != 0)
    }
}

#[derive(Debug)]
pub(crate) struct OwnedLease(Arc<LeaseLife>);

impl OwnedLease {
    pub(crate) fn new(scope: Option<&LeaseScope>) -> Self {
        Self(Arc::new(LeaseLife {
            scope: scope.map(|scope| Arc::downgrade(&scope.0)),
        }))
    }

    pub(crate) fn is_active(&self) -> bool {
        self.0.is_active()
    }
}

#[derive(Debug, Default)]
struct Entry {
    borrowed: usize,
    owned: Vec<Weak<LeaseLife>>,
}

impl Entry {
    fn prune(&mut self) {
        self.owned
            .retain(|lease| lease.upgrade().is_some_and(|lease| lease.is_active()));
    }

    fn is_active(&self) -> bool {
        self.borrowed != 0
            || self
                .owned
                .iter()
                .any(|lease| lease.upgrade().is_some_and(|lease| lease.is_active()))
    }
}

#[derive(Debug, Default)]
pub(crate) struct LeaseTable {
    entries: RefCell<HashMap<HeapObjectId, Entry>>,
}

pub(crate) struct BorrowedLease<'table> {
    table: &'table LeaseTable,
    id: HeapObjectId,
}

impl Drop for BorrowedLease<'_> {
    fn drop(&mut self) {
        let mut entries = self.table.entries.borrow_mut();
        let entry = entries.get_mut(&self.id).expect("registered storage lease");
        entry.borrowed -= 1;
        entry.prune();
        if entry.borrowed == 0 && entry.owned.is_empty() {
            entries.remove(&self.id);
        }
    }
}

impl LeaseTable {
    pub(crate) fn is_active(&self, id: HeapObjectId) -> bool {
        self.entries.borrow().get(&id).is_some_and(Entry::is_active)
    }

    pub(crate) fn acquire(
        &self,
        id: HeapObjectId,
        scope: Option<&LeaseScope>,
    ) -> Result<OwnedLease, RuntimeError> {
        let mut entries = self.entries.borrow_mut();
        entries
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("storage lease registry"))?;
        let entry = entries.entry(id).or_default();
        entry.prune();
        entry
            .owned
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("storage lease capacity"))?;
        let lease = OwnedLease::new(scope);
        entry.owned.push(Arc::downgrade(&lease.0));
        Ok(lease)
    }

    /// No per-call allocation after the keyed table is warm.
    pub(crate) fn borrow(&self, id: HeapObjectId) -> Result<BorrowedLease<'_>, RuntimeError> {
        let mut entries = self.entries.borrow_mut();
        entries
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("storage lease registry"))?;
        let entry = entries.entry(id).or_default();
        entry.borrowed = entry
            .borrowed
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("storage lease depth"))?;
        Ok(BorrowedLease { table: self, id })
    }

    pub(crate) fn prune(&self) {
        self.entries.borrow_mut().retain(|_, entry| {
            entry.prune();
            entry.borrowed != 0 || !entry.owned.is_empty()
        });
    }
}
