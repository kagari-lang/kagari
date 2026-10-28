//! Inert taken identities own stable occupied slots, separate from heap generations.
use crate::{
    error::RuntimeError,
    gc::{
        HeapObjectId,
        graph::{
            GraphMarker,
            marks::{Domain, MarkBlocks, MarkRef},
        },
    },
    native::storage::scan::{ScanBudget, ScanStatus},
};
use std::collections::HashMap;

#[derive(Debug)]
struct Slot {
    identity: Option<HeapObjectId>,
    previous: Option<usize>,
    next: Option<usize>,
}

#[derive(Debug)]
struct Sweep {
    next: Option<usize>,
    remaining_units: usize,
}

#[derive(Debug, Default)]
pub(super) struct MovedLedger {
    by_id: HashMap<HeapObjectId, usize>,
    slots: Vec<Slot>,
    marks: MarkBlocks,
    free: Vec<usize>,
    first: Option<usize>,
    sweep: Option<Sweep>,
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

    /// Capture the old occupied suffix. Native takes prepend newborn identities
    /// without extending this cursor, even when they reuse a retired slot.
    pub(super) fn begin_sweep(&mut self, available_units: usize) -> Result<(), RuntimeError> {
        if self.sweep.is_some() {
            return Err(invalid());
        }
        self.sweep = Some(Sweep {
            next: self.first,
            remaining_units: available_units,
        });
        Ok(())
    }

    /// Return exact retired identity units for immediate resource release by the
    /// driver. Inspection plus unlink/credit/accounting work is reserved first;
    /// fewer than four available units cannot retire a dead owner.
    pub(super) fn sweep_slice(
        &mut self,
        graph: &mut GraphMarker<'_>,
        budget: &mut ScanBudget,
    ) -> Result<(ScanStatus, usize), RuntimeError> {
        let graph = graph.sweep_batch()?;
        let mut next = self.sweep.as_ref().ok_or_else(invalid)?.next;
        let mut dead = [None; 32];
        let mut count = 0;
        while let Some(index) = next {
            if count == dead.len() || !budget.take(1) {
                break;
            }
            let slot = self.slots.get(index).ok_or_else(invalid)?;
            let id = slot.identity.ok_or_else(invalid)?;
            if self.by_id.get(&id) != Some(&index) {
                return Err(invalid());
            }
            if !graph.is_survivor(self.marks.slot(Domain::Moved, index))? {
                if !budget.take(3) {
                    break;
                }
                dead[count] = Some((index, id));
                count += 1;
            }
            next = slot.next;
        }
        let remaining_units = self
            .sweep
            .as_ref()
            .ok_or_else(invalid)?
            .remaining_units
            .checked_sub(count)
            .ok_or_else(invalid)?;
        let credits = graph.prepare_retirement(count)?;
        let sweep = self.sweep.as_mut().expect("active moved sweep");
        sweep.next = next;
        sweep.remaining_units = remaining_units;
        for (index, id) in dead[..count].iter().flatten().copied() {
            self.remove(index, id);
        }
        credits.commit();
        let status = if next.is_none() {
            self.sweep = None;
            ScanStatus::Complete
        } else {
            ScanStatus::Pending
        };
        Ok((status, count))
    }

    fn remove(&mut self, index: usize, id: HeapObjectId) {
        let slot = &self.slots[index];
        let (previous, next) = (slot.previous, slot.next);
        if let Some(previous) = previous {
            self.slots[previous].next = next;
        } else {
            self.first = next;
        }
        if let Some(next) = next {
            self.slots[next].previous = previous;
        }
        if let Some(sweep) = &mut self.sweep
            && sweep.next == Some(index)
        {
            sweep.next = next;
        }
        self.slots[index].identity = None;
        self.by_id.remove(&id);
        self.free.push(index);
    }
}

fn exhausted() -> RuntimeError {
    RuntimeError::resource_limit("moved identity ledger")
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid moved sweep cursor or identity")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::graph::{GraphIdentity, GraphState};

    fn insert(ledger: &mut MovedLedger, graph: &mut GraphMarker<'_>, id: HeapObjectId) {
        ledger.prepare(id).unwrap();
        graph.prepare_identity().unwrap();
        graph.admit_prepared(GraphIdentity::Moved(id), ledger.insert_prepared(id));
    }

    #[test]
    fn retirement_preflights_logical_units_and_caps_actual_owners() {
        let state = GraphState::default();
        let mut graph = state.lock();
        let mut ledger = MovedLedger::default();
        let ids: Vec<_> = (0..33).map(|slot| HeapObjectId::new(1, slot, 0)).collect();
        for &id in &ids {
            insert(&mut ledger, &mut graph, id);
        }
        graph.begin_cycle().unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        ledger.begin_sweep(31).unwrap();
        let first = ledger.sweep.as_ref().unwrap().next;
        assert!(
            ledger
                .sweep_slice(&mut graph, &mut ScanBudget::new(1024))
                .is_err()
        );
        assert_eq!(ledger.sweep.as_ref().unwrap().next, first);
        assert!(
            ids.iter().all(|id| ledger.contains_key(id)),
            "failed aggregate preflight cannot unlink or retire credits"
        );
        ledger.sweep.as_mut().unwrap().remaining_units = 33;
        let mut budget = ScanBudget::new(1024);
        assert_eq!(
            ledger.sweep_slice(&mut graph, &mut budget).unwrap(),
            (ScanStatus::Pending, 32)
        );
        assert_eq!(budget.spent(), 128);
        assert_eq!(ledger.by_id.len(), 1);
        assert!(ledger.contains_key(&ids[0]));
        assert_eq!(
            ledger
                .sweep_slice(&mut graph, &mut ScanBudget::new(4))
                .unwrap(),
            (ScanStatus::Complete, 1)
        );
        assert!(ledger.is_empty());
        graph.finish_sweep().unwrap();
    }

    #[test]
    fn occupied_cursor_keeps_full_identity_and_excludes_reused_newborn_slots() {
        let state = GraphState::default();
        let mut graph = state.lock();
        let mut ledger = MovedLedger::default();
        let ids = [
            HeapObjectId::new(1, 2, 3),
            HeapObjectId::new(1, 5, 3),
            HeapObjectId::new(1, 2, 4),
        ];
        for id in ids {
            insert(&mut ledger, &mut graph, id);
        }
        let stable = ledger.by_id[&ids[0]];
        let reused = ledger.by_id[&ids[2]];
        graph.begin_cycle().unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        ledger.begin_sweep(3).unwrap();
        let mut too_small = ScanBudget::new(1);
        assert_eq!(
            ledger.sweep_slice(&mut graph, &mut too_small).unwrap(),
            (ScanStatus::Pending, 0)
        );
        assert_eq!(too_small.spent(), 1);
        assert_eq!(ledger.by_id.len(), 3);
        let mut budget = ScanBudget::new(4);
        assert_eq!(
            ledger.sweep_slice(&mut graph, &mut budget).unwrap(),
            (ScanStatus::Pending, 1)
        );
        assert_eq!(budget.spent(), 4);
        assert!(!ledger.contains_key(&ids[2]));
        assert_eq!(ledger.by_id[&ids[0]], stable);
        let newborn = HeapObjectId::new(1, 2, 5);
        insert(&mut ledger, &mut graph, newborn);
        assert_eq!(ledger.by_id[&newborn], reused);
        assert!(ledger.prepare(newborn).is_err());
        let mut retired = 1;
        loop {
            let (status, count) = ledger
                .sweep_slice(&mut graph, &mut ScanBudget::new(4))
                .unwrap();
            retired += count;
            if status == ScanStatus::Complete {
                break;
            }
        }
        assert_eq!(retired, 3);
        assert_eq!(ledger.by_id.len(), 1);
        assert!(ledger.contains_key(&newborn));
        graph.finish_sweep().unwrap();
        graph.begin_cycle().unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        ledger.begin_sweep(1).unwrap();
        assert_eq!(
            ledger
                .sweep_slice(&mut graph, &mut ScanBudget::new(4))
                .unwrap(),
            (ScanStatus::Complete, 1)
        );
        assert!(ledger.is_empty());
    }
}
