//! Original-page colors and one reserved, deduplicated work queue.

use crate::{
    allocation::{PhysicalHeap, Reservation},
    descriptor::Descriptor,
    error::GcError,
    heap::Phase,
    object::ObjectRef,
    root::RootId,
    scan::{ScanBudget, ScanKind, ScanStatus},
    table::shrink_vec,
};
use std::{cell::Cell, sync::Arc};

#[derive(Debug, Default)]
pub(crate) struct PageMarks {
    epoch: Cell<u64>,
    discovered: [Cell<u64>; 2],
    scanned: [Cell<u64>; 2],
    queued: Cell<bool>,
}

impl PageMarks {
    pub(crate) fn reset_needed(&self, epoch: u64) -> bool {
        self.epoch.get() != epoch
    }

    pub(crate) fn ensure(&self, epoch: u64) {
        if self.reset_needed(epoch) {
            for word in &self.discovered {
                word.set(0);
            }
            for word in &self.scanned {
                word.set(0);
            }
            self.queued.set(false);
            self.epoch.set(epoch);
        }
    }

    pub(crate) fn black(&self, slot: usize, epoch: u64) -> bool {
        self.black_word(slot / 64, epoch) & (1 << (slot % 64)) != 0
    }

    pub(crate) fn black_word(&self, word: usize, epoch: u64) -> u64 {
        if self.reset_needed(epoch) {
            0
        } else {
            self.scanned[word].get()
        }
    }

    fn discovered(&self, slot: usize, epoch: u64) -> bool {
        !self.reset_needed(epoch) && self.discovered[slot / 64].get() & (1 << (slot % 64)) != 0
    }

    pub(crate) fn set_black(&self, slot: usize, epoch: u64) {
        self.ensure(epoch);
        let bit = 1 << (slot % 64);
        self.discovered[slot / 64].set(self.discovered[slot / 64].get() | bit);
        self.scanned[slot / 64].set(self.scanned[slot / 64].get() | bit);
    }

    pub(crate) fn clear_slot(&self, slot: usize) {
        let bit = !(1 << (slot % 64));
        self.discovered[slot / 64].set(self.discovered[slot / 64].get() & bit);
        self.scanned[slot / 64].set(self.scanned[slot / 64].get() & bit);
    }

    fn gray(&self, epoch: u64) -> u128 {
        if self.reset_needed(epoch) {
            return 0;
        }
        u128::from(self.discovered[0].get() & !self.scanned[0].get())
            | (u128::from(self.discovered[1].get() & !self.scanned[1].get()) << 64)
    }
}

#[derive(Debug)]
pub(crate) struct ActiveScan {
    pub(crate) owner: ObjectRef,
    pub(crate) descriptor: Arc<Descriptor>,
    pub(crate) restart: bool,
}

#[derive(Debug, Default)]
pub(crate) struct RootCursor {
    pub(crate) end: usize,
    pub(crate) next: usize,
    pub(crate) target: usize,
    pub(crate) active: Option<RootId>,
}

#[derive(Debug)]
pub(crate) struct Marking {
    pub(crate) phase: Phase,
    pub(crate) epoch: u64,
    pending: Vec<u32>,
    pub(crate) cycles: u64,
    pub(crate) publication_work: usize,
}

impl Default for Marking {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            epoch: 0,
            pending: Vec::new(),
            cycles: 0,
            publication_work: 0,
        }
    }
}

impl Marking {
    pub(crate) fn check(&self) -> Result<(), GcError> {
        match self.phase {
            Phase::Faulted => Err(GcError::Faulted),
            Phase::Closed => Err(GcError::Closed),
            _ => Ok(()),
        }
    }

    pub(crate) fn reserve_pages(&mut self, count: usize) -> Result<(), GcError> {
        self.pending
            .try_reserve(count.saturating_sub(self.pending.len()))?;
        Ok(())
    }

    pub(crate) fn trim(&mut self, pages: usize) {
        assert_eq!(self.phase, Phase::Idle);
        assert!(self.pending.is_empty());
        shrink_vec(&mut self.pending, pages);
    }

    pub(crate) fn buffer_bytes(&self) -> usize {
        self.pending.capacity() * size_of::<u32>()
    }

    pub(crate) fn close(&mut self) {
        self.phase = Phase::Closed;
        self.pending = Vec::new();
    }

    pub(crate) fn start(&mut self) -> Result<(), GcError> {
        self.check()?;
        assert_eq!(self.phase, Phase::Idle);
        assert!(self.pending.is_empty());
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(GcError::Limit("mark epoch"))?;
        self.phase = Phase::Roots;
        Ok(())
    }

    pub(crate) fn admit(
        &mut self,
        physical: &PhysicalHeap,
        reservation: Reservation,
    ) -> Result<(), GcError> {
        if self.phase != Phase::Idle {
            // All outgoing targets were already published from the initialized value.
            physical
                .page(reservation.page)?
                .marks()
                .set_black(reservation.slot, self.epoch);
        }
        Ok(())
    }

    pub(crate) fn publish(
        &mut self,
        physical: &PhysicalHeap,
        reference: ObjectRef,
        budget: Option<&mut ScanBudget>,
    ) -> Result<ScanStatus, GcError> {
        self.check()?;
        let object = physical.locate(reference)?;
        if self.phase == Phase::Idle {
            return Ok(ScanStatus::Complete);
        }
        let marks = object.page.marks();
        if self.phase == Phase::Sweep {
            return if marks.black(object.slot, self.epoch) {
                Ok(ScanStatus::Complete)
            } else {
                Err(GcError::Storage("publication of condemned object"))
            };
        }
        if marks.discovered(object.slot, self.epoch) {
            return Ok(ScanStatus::Complete);
        }
        let reset = usize::from(marks.reset_needed(self.epoch)) * 4;
        if let Some(budget) = budget {
            if !budget.take(reset) {
                return Ok(ScanStatus::Pending);
            }
        } else {
            self.publication_work = self
                .publication_work
                .checked_add(1 + reset)
                .ok_or(GcError::Limit("publication work"))?;
        }
        // Only newly discovered objects need dispatch classification. A retained
        // source and occupied original slot already prove a valid descriptor.
        let leaf = physical.descriptors.get(object.descriptor)?.kind == ScanKind::EdgeFree;
        if !leaf && !marks.queued.get() && self.pending.len() == self.pending.capacity() {
            return Err(GcError::Storage("unreserved mark job"));
        }
        marks.ensure(self.epoch);
        let bit = 1 << (object.slot % 64);
        marks.discovered[object.slot / 64].set(marks.discovered[object.slot / 64].get() | bit);
        if leaf {
            marks.set_black(object.slot, self.epoch);
        } else if !marks.queued.replace(true) {
            self.pending.push(object.id);
        }
        Ok(ScanStatus::Complete)
    }

    pub(crate) fn next(&mut self, physical: &PhysicalHeap) -> Result<Option<ObjectRef>, GcError> {
        if let Some(id) = self.pending.pop() {
            let page = physical.page(id)?;
            let marks = page.marks();
            marks.queued.set(false);
            let gray = marks.gray(self.epoch);
            if gray == 0 {
                return Ok(None);
            }
            let slot = gray.trailing_zeros() as usize;
            if gray & (gray - 1) != 0 {
                marks.queued.set(true);
                self.pending.push(id);
            }
            if !page.occupied(slot) {
                return Err(GcError::Storage("gray free slot"));
            }
            return Ok(Some(ObjectRef(page.pointer(slot))));
        }
        Ok(None)
    }

    pub(crate) fn has_jobs(&self) -> bool {
        !self.pending.is_empty()
    }
}
