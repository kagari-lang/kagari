//! One collection state machine for incremental service, draining and shutdown.

use crate::{
    allocation::PhysicalCursor,
    error::GcError,
    heap::{Heap, Phase},
    marking::{ActiveScan, RootCursor},
    root::RootId,
    root_source::RootScan,
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
    sweep::{RETIREMENT_CAP, RetirementBatch},
};
use std::sync::Arc;

#[derive(Debug, Default)]
pub(super) enum SweepStage {
    #[default]
    Physical,
    Roots {
        remaining: usize,
    },
    Descriptors {
        remaining: usize,
    },
}

impl Heap {
    pub(super) fn service(
        &mut self,
        budget: &mut ScanBudget,
        source: &mut impl RootScan,
    ) -> Result<usize, GcError> {
        if self.marking.phase == Phase::Idle {
            if !budget.take(1) {
                return Ok(0);
            }
            self.marking.start()?;
            source.begin()?;
            self.ledger.allocation_growth = 0;
            self.root_cursor = RootCursor {
                end: self.roots.active.len(),
                ..RootCursor::default()
            };
        }
        while budget.remaining() != 0 {
            match self.marking.phase {
                Phase::Roots => {
                    if !self.scan_roots(budget, source)? {
                        break;
                    }
                }
                Phase::Mark => {
                    if !self.scan_object(budget)? {
                        break;
                    }
                }
                Phase::Sweep => return self.sweep(budget),
                Phase::Idle => break,
                Phase::Faulted | Phase::Closed => return self.marking.check().map(|_| 0),
            }
        }
        Ok(0)
    }

    fn scan_roots(
        &mut self,
        budget: &mut ScanBudget,
        source: &mut impl RootScan,
    ) -> Result<bool, GcError> {
        if self.root_cursor.next >= self.root_cursor.end {
            let mut visitor = EdgeVisitor {
                physical: &self.physical,
                marking: &mut self.marking,
            };
            if source.scan(budget, &mut visitor)? == ScanStatus::Pending {
                return Ok(false);
            }
            if !budget.take(1) {
                return Ok(false);
            }
            self.marking.phase = Phase::Mark;
            return Ok(true);
        }
        if !budget.take(1) {
            return Ok(false);
        }
        // Releases are consumed only during Sweep; additions append beyond end.
        let next = self.roots.active[self.root_cursor.next];
        let slot = &self.roots.slots[next];
        let id = RootId {
            heap: self.id,
            slot: next,
            generation: slot.generation,
        };
        let valid = self.root_cursor.active.is_none_or(|active| active == id);
        let entry = (valid && slot.entry.lease.strong_count() != 0).then_some(&slot.entry);
        if let Some(entry) = entry
            && self.root_cursor.target < entry.targets.len()
        {
            self.root_cursor.active = Some(id);
            if let Some(target) = entry.targets[self.root_cursor.target]
                && self.marking.publish(&self.physical, target, Some(budget))?
                    == ScanStatus::Pending
            {
                return Ok(false);
            }
            self.root_cursor.target += 1;
            return Ok(true);
        }
        self.root_cursor.next += 1;
        self.root_cursor.target = 0;
        self.root_cursor.active = None;
        Ok(true)
    }

    fn scan_object(&mut self, budget: &mut ScanBudget) -> Result<bool, GcError> {
        let object = if let Some(active) = &self.active {
            self.physical.resolve(active.owner)?
        } else {
            if !budget.take(4) {
                return Ok(false);
            }
            if let Some(owner) = self.marking.next(&self.physical)? {
                let object = self.physical.resolve(owner)?;
                let descriptor = object.descriptor.clone();
                // SAFETY: registered aligned workspace, current live owner and exact dispatch.
                unsafe { descriptor.begin(object.pointer, self.workspace.pointer()) };
                self.active = Some(ActiveScan {
                    owner,
                    descriptor,
                    restart: false,
                });
                object
            } else if !self.marking.has_jobs() {
                self.marking.phase = Phase::Sweep;
                self.physical.cursor = PhysicalCursor {
                    page: self.physical.first_active,
                    slot: 0,
                };
                self.sweep_stage = SweepStage::Physical;
                return Ok(true);
            } else {
                return Ok(true);
            }
        };
        let active = self.active.as_mut().expect("active scanner");
        if !Arc::ptr_eq(object.descriptor, &active.descriptor) {
            return Err(GcError::Storage("active descriptor changed"));
        }
        if active.restart {
            // SAFETY: old Copy state is discarded; exact descriptor initializes replacement.
            unsafe {
                active
                    .descriptor
                    .begin(object.pointer, self.workspace.pointer())
            };
            active.restart = false;
        }
        if let ScanKind::Bounded { max_work } = active.descriptor.kind
            && budget.remaining() < max_work * 5
        {
            return Ok(false);
        }
        let before = budget.spent();
        let available = budget.remaining();
        let mut visitor = EdgeVisitor {
            physical: &self.physical,
            marking: &mut self.marking,
        };
        // SAFETY: owner, initialized state and split visitor services are held through call.
        let status = unsafe {
            active.descriptor.scan(
                object.pointer,
                self.workspace.pointer(),
                budget,
                &mut visitor,
            )?
        };
        let spent = budget.spent() - before;
        if active.descriptor.kind != ScanKind::Indivisible && spent > available {
            return Err(GcError::Provider("bounded scan exceeded allowance"));
        }
        if matches!(active.descriptor.kind, ScanKind::Bounded { max_work } if status != ScanStatus::Complete || spent > max_work * 5)
        {
            return Err(GcError::Provider("bounded scan violated declared bound"));
        }
        match status {
            ScanStatus::Pending => {
                if spent == 0 && available >= 5 {
                    return Err(GcError::Provider("scanner made no progress"));
                }
                Ok(false)
            }
            ScanStatus::Complete => {
                self.physical
                    .page(object.page)?
                    .marks()
                    .set_black(object.slot, self.marking.epoch);
                self.active = None;
                Ok(true)
            }
        }
    }

    fn sweep(&mut self, budget: &mut ScanBudget) -> Result<usize, GcError> {
        let mut batch = RetirementBatch::new();
        while batch.len() < RETIREMENT_CAP && budget.take(1) {
            match self.sweep_stage {
                SweepStage::Physical => {
                    let Some(id) = self.physical.cursor.page else {
                        self.sweep_stage = SweepStage::Roots {
                            remaining: self.roots.pending_releases(),
                        };
                        continue;
                    };
                    let page = self.physical.page(id)?;
                    let slot = self.physical.cursor.slot;
                    if slot >= page.slots() {
                        self.physical.cursor = PhysicalCursor {
                            page: page.active().next,
                            slot: 0,
                        };
                        continue;
                    }
                    // Dense dead runs already have their next owner; bitmap
                    // search is useful only when skipping live/unoccupied slots.
                    let (next, dead) =
                        if page.occupied(slot) && !page.marks().black(slot, self.marking.epoch) {
                            (slot + 1, Some(slot))
                        } else {
                            let end = page
                                .slots()
                                .min(slot + budget.remaining().min(page.slots()) + 1);
                            let dead = page.next_dead(slot, end, self.marking.epoch);
                            if dead < end {
                                (dead + 1, Some(dead))
                            } else {
                                (end, None)
                            }
                        };
                    // The loop prepaid one unit; charge all remaining skipped
                    // slots before advancing, preserving tiny-step accounting.
                    assert!(budget.take(next - slot - 1));
                    self.physical.cursor.slot = next;
                    if let Some(dead) = dead {
                        let charge = batch.detach(
                            &mut self.physical,
                            id,
                            dead,
                            Some(self.ledger.indirect),
                        )?;
                        self.ledger.indirect -= charge;
                    }
                }
                SweepStage::Roots { remaining } => {
                    if remaining != 0 {
                        let entry = self
                            .roots
                            .release_one()
                            .ok_or(GcError::Storage("root release snapshot exhausted"))?;
                        batch.push_root(entry);
                        self.sweep_stage = SweepStage::Roots {
                            remaining: remaining - 1,
                        };
                    } else {
                        self.sweep_stage = SweepStage::Descriptors {
                            remaining: self.physical.descriptors.len(),
                        };
                    }
                }
                SweepStage::Descriptors { remaining } => {
                    // Deferred owners still pin descriptors until this batch is disposed.
                    if batch.len() != 0 {
                        break;
                    }
                    if remaining != 0 {
                        // Walk backwards: swap-removal only moves an already
                        // visited (or newly appended) entry into this position.
                        self.physical.descriptors.retire_unused(remaining - 1);
                        self.sweep_stage = SweepStage::Descriptors {
                            remaining: remaining - 1,
                        };
                    } else {
                        self.marking.cycles = self
                            .marking
                            .cycles
                            .checked_add(1)
                            .ok_or(GcError::Limit("completed cycles"))?;
                        self.marking.phase = Phase::Idle;
                        break;
                    }
                }
            }
        }
        let count = batch.len();
        if batch.finish(&mut self.physical, || self.marking.phase = Phase::Faulted)? != 0 {
            return Err(GcError::Provider("object destructor panicked"));
        }
        Ok(count)
    }

    pub(super) fn dispose_all(&mut self) -> (usize, usize) {
        self.physical.cursor = PhysicalCursor {
            page: self.physical.first_active,
            slot: 0,
        };
        let mut disposed = 0;
        let mut panics = 0;
        loop {
            let mut batch = RetirementBatch::new();
            while batch.len() < RETIREMENT_CAP {
                let Some(id) = self.physical.cursor.page else {
                    break;
                };
                let page = self.physical.page(id).expect("owned shutdown page");
                let slot = self.physical.cursor.slot;
                if slot >= page.slots() {
                    self.physical.cursor = PhysicalCursor {
                        page: page.active().next,
                        slot: 0,
                    };
                    continue;
                }
                let retire = page.occupied(slot);
                self.physical.cursor.slot += 1;
                if retire {
                    batch
                        .detach(&mut self.physical, id, slot, None)
                        .expect("owned initialized shutdown object");
                }
            }
            disposed += batch.len();
            panics += batch
                .finish(&mut self.physical, || {})
                .expect("linear shutdown retirement");
            if self.physical.cursor.page.is_none() {
                break;
            }
        }
        (disposed, panics)
    }
}
