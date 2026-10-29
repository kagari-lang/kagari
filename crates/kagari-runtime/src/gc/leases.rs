//! Heap-owned exclusion records. Guards never own storage or execution state.
use crate::{error::RuntimeError, gc::HeapObjectId, native::views::NativeOperationKind};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

/// Exclude collection, script reentry and competing storage access during a loan.
/// The caller finishes ordered admission before constructing this guard.
pub(super) struct StorageBorrow<'heap>(&'heap Cell<usize>);

impl<'heap> StorageBorrow<'heap> {
    pub(super) fn enter(borrows: &'heap Cell<usize>) -> Self {
        borrows.set(borrows.get() + 1);
        Self(borrows)
    }
}

impl Drop for StorageBorrow<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

#[derive(Debug, Default)]
struct Entry {
    borrowed: usize,
    operation: Option<NativeOperationKind>,
}

impl Entry {
    fn is_active(&self) -> bool {
        self.borrowed != 0
    }
}

/// Entries exist exactly while at least one guard is outstanding. The final
/// guard removes its entry, so collection never needs to scan this table.
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
        if entry.borrowed == 0 {
            entries.remove(&self.id);
        }
    }
}

impl LeaseTable {
    pub(crate) fn operation(&self, id: HeapObjectId) -> Option<NativeOperationKind> {
        self.entries
            .borrow()
            .get(&id)
            .and_then(|entry| entry.operation)
    }

    pub(crate) fn borrow_operation(
        &self,
        id: HeapObjectId,
        kind: NativeOperationKind,
    ) -> Result<BorrowedLease<'_>, RuntimeError> {
        let lease = self.borrow(id)?;
        self.entries
            .borrow_mut()
            .get_mut(&id)
            .expect("admitted operation")
            .operation = Some(kind);
        Ok(lease)
    }

    pub(crate) fn is_active(&self, id: HeapObjectId) -> bool {
        self.entries.borrow().get(&id).is_some_and(Entry::is_active)
    }

    /// No per-call allocation after the keyed table is warm.
    pub(crate) fn borrow(&self, id: HeapObjectId) -> Result<BorrowedLease<'_>, RuntimeError> {
        let mut entries = self.entries.borrow_mut();
        entries
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("storage lease registry"))?;
        // A new zero-count entry always admits its first guard. Overflow can
        // only reject an existing nonzero entry and leaves its guards intact.
        let entry = entries.entry(id).or_default();
        entry.borrowed = entry
            .borrowed
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("storage lease depth"))?;
        Ok(BorrowedLease { table: self, id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn failed_nested_borrow_and_unwind_preserve_final_guard_removal() {
        let table = LeaseTable::default();
        let id = HeapObjectId {
            owner: 7,
            slot: 0,
            generation: 0,
        };
        let first = table
            .borrow_operation(id, NativeOperationKind::ExclusiveSlots)
            .unwrap();
        let nested = table.borrow(id).unwrap();
        drop(first);
        assert!(table.is_active(id));
        assert_eq!(
            table.operation(id),
            Some(NativeOperationKind::ExclusiveSlots)
        );
        // Capacity exhaustion must leave the existing registration intact.
        table.entries.borrow_mut().get_mut(&id).unwrap().borrowed = usize::MAX;
        assert!(table.borrow(id).is_err());
        assert_eq!(table.entries.borrow()[&id].borrowed, usize::MAX);
        table.entries.borrow_mut().get_mut(&id).unwrap().borrowed = 1;
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _nested = nested;
                panic!("unwind the final lease");
            }))
            .is_err()
        );
        assert!(!table.is_active(id));
        assert!(table.operation(id).is_none());
        assert!(table.entries.borrow().is_empty());
        let next = table.borrow(id).unwrap();
        assert!(table.operation(id).is_none());
        drop(next);
        assert!(table.entries.borrow().is_empty());
    }
}
