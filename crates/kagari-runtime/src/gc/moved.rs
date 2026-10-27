//! Inert taken identities own stable occupied slots, separate from heap generations.
use crate::{
    error::RuntimeError,
    gc::{
        HeapObjectId,
        graph::marks::{Domain, MarkBlocks, MarkRef},
    },
};
use std::{collections::HashMap, iter};

#[derive(Debug)]
struct Slot {
    identity: Option<HeapObjectId>,
    previous: Option<usize>,
    next: Option<usize>,
}

#[derive(Debug, Default)]
pub(super) struct MovedLedger {
    by_id: HashMap<HeapObjectId, usize>,
    slots: Vec<Slot>,
    marks: MarkBlocks,
    free: Vec<usize>,
    first: Option<usize>,
}

impl MovedLedger {
    /// Reserve lookup, occupied slot and retirement capacity before native detach.
    pub(super) fn prepare(&mut self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.by_id.contains_key(&id) {
            return Err(RuntimeError::module_validation("duplicate moved identity"));
        }
        self.by_id.try_reserve(1).map_err(|_| exhausted())?;
        self.marks
            .prepare(self.free.last().copied().unwrap_or(self.slots.len()))
            .map_err(|_| exhausted())?;
        if self.free.is_empty() {
            self.slots.try_reserve(1).map_err(|_| exhausted())?;
            self.free
                .try_reserve(self.slots.len() + 1 - self.free.len())
                .map_err(|_| exhausted())?;
        }
        Ok(())
    }

    pub(super) fn insert_prepared(&mut self, id: HeapObjectId) -> MarkRef<'_> {
        assert!(!self.by_id.contains_key(&id));
        assert!(self.by_id.len() < self.by_id.capacity());
        let slot = Slot {
            identity: Some(id),
            previous: None,
            next: self.first,
        };
        let index = if let Some(index) = self.free.pop() {
            self.slots[index] = slot;
            index
        } else {
            assert!(self.slots.len() < self.slots.capacity());
            let index = self.slots.len();
            self.slots.push(slot);
            index
        };
        if let Some(first) = self.first {
            self.slots[first].previous = Some(index);
        }
        self.first = Some(index);
        self.by_id.insert(id, index);
        self.marks.slot(Domain::Moved, index)
    }

    pub(super) fn get(&self, id: &HeapObjectId) -> Option<MarkRef<'_>> {
        let index = *self.by_id.get(id)?;
        let slot = self.slots.get(index)?;
        (slot.identity == Some(*id)).then(|| self.marks.slot(Domain::Moved, index))
    }

    pub(super) fn next_gray(&self, block: usize, epoch: u64) -> (Option<HeapObjectId>, bool) {
        let (index, more) = self.marks.next_gray(block, epoch);
        (
            index.map(|index| {
                self.slots[index]
                    .identity
                    .expect("gray moved slot occupied")
            }),
            more,
        )
    }

    pub(super) fn contains_key(&self, id: &HeapObjectId) -> bool {
        self.get(id).is_some()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    /// Scoped synchronous sweep bridge; no iterator survives a collector step.
    pub(super) fn values(&self) -> impl Iterator<Item = MarkRef<'_>> {
        iter::successors(self.first, |index| self.slots[*index].next)
            .map(|index| self.marks.slot(Domain::Moved, index))
    }

    /// PG05 replaces this synchronous loop with an owned slot cursor. Occupied
    /// links already avoid historical vacancies and never renumber another owner.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(HeapObjectId, MarkRef<'_>) -> bool) {
        let mut next = self.first;
        while let Some(index) = next {
            let slot = &self.slots[index];
            next = slot.next;
            let id = slot.identity.expect("occupied moved slot");
            if keep(id, self.marks.slot(Domain::Moved, index)) {
                continue;
            }
            let previous = slot.previous;
            if let Some(previous) = previous {
                self.slots[previous].next = next;
            } else {
                self.first = next;
            }
            if let Some(next) = next {
                self.slots[next].previous = previous;
            }
            self.slots[index].identity = None;
            self.by_id.remove(&id);
            self.free.push(index);
        }
    }
}

fn exhausted() -> RuntimeError {
    RuntimeError::resource_limit("moved identity ledger")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occupied_slots_survive_neighbor_retirement_and_full_identity_reuse() {
        let mut ledger = MovedLedger::default();
        let first = HeapObjectId {
            owner: 1,
            slot: 2,
            generation: 3,
        };
        let next_generation = HeapObjectId {
            generation: 4,
            ..first
        };
        let other = HeapObjectId { slot: 5, ..first };
        for id in [first, other, next_generation] {
            ledger.prepare(id).unwrap();
            ledger.insert_prepared(id);
        }
        let stable = ledger.by_id[&first];
        let reused = ledger.by_id[&other];
        ledger.retain(|id, _| id != other);
        assert_eq!(ledger.by_id[&first], stable);
        assert!(ledger.contains_key(&first));
        assert!(ledger.contains_key(&next_generation));
        assert_eq!(ledger.values().count(), 2);
        assert!(ledger.prepare(first).is_err());
        ledger.prepare(other).unwrap();
        ledger.insert_prepared(other);
        assert_eq!(ledger.by_id[&other], reused);
        ledger.retain(|_, _| false);
        assert!(ledger.is_empty());
        assert_eq!(ledger.values().count(), 0);
    }
}
