//! Bounded multi-fragment prepare/commit with one aggregate identity/charge proof.
pub(in crate::gc) mod disposal;
#[cfg(test)]
mod mark_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transaction_tests;

use self::disposal::SweepBatch;
use super::{
    ObjectStorage,
    pages::batch::{Decision, Selection, order::ordering_work},
};
use crate::{
    execution_metadata::interfaces::InterfaceStore,
    gc::{
        graph::{GraphMarker, MarkPhase},
        reference::GcRef,
    },
    native::storage::scan::ScanBudget,
};
use std::{cell::RefCell, mem};

pub(in crate::gc) const RETIRE_LIMIT: usize = 32;
// Three fixed arrays, output-empty preflight, final empty token-array cleanup,
// and two selection-local bitmap words. Reserve before any initialization,
// including progress-only and failed preparation.
const WORKSPACE_WORK: usize = RETIRE_LIMIT * 5 + 2;
const OWNER_RETIRE_WORK: usize = 8;
// Bitmap checks, physical detach/finish, descriptor counts and final-owner release.
// Final-page finish also checks two gray words and unregisters one mark credit;
// this constant allowance covers that bounded control work, without a history walk.
const FRAGMENT_FINISH_WORK: usize = 32;
const BATCH_FINISH_WORK: usize = 32;

#[derive(Debug)]
pub(super) struct Sweep {
    outstanding: bool,
    max_units: usize,
    max_bytes: u128,
    totals: SweepTotals,
}

#[derive(Debug, Default, Clone, Copy)]
pub(in crate::gc) struct SweepTotals {
    pub(in crate::gc) objects: usize,
    pub(in crate::gc) units: usize,
    pub(in crate::gc) bytes: u128,
    pub(in crate::gc) live: usize,
}

struct Prepared {
    identities: [Option<GcRef>; RETIRE_LIMIT],
    count: usize,
    units: usize,
    bytes: u128,
}

/// Only borrowed prefixes leave preparation; the large arrays stay in place.
struct Commit<'a> {
    identities: &'a [Option<GcRef>],
    units: usize,
    bytes: u128,
}

impl Prepared {
    #[inline(always)]
    fn new() -> Self {
        Self {
            identities: [None; RETIRE_LIMIT],
            count: 0,
            units: 0,
            bytes: 0,
        }
    }

    fn push(&mut self, identity: GcRef, units: usize, bytes: usize) -> Option<()> {
        self.units = self.units.checked_add(units)?;
        self.bytes = self.bytes.checked_add(bytes as u128)?;
        self.identities[self.count] = Some(identity);
        self.count += 1;
        Some(())
    }

    /// Invalidate before the first mutation. An abandoned or failed page preflight
    /// cannot replay this prefix, and no large workspace is moved into commit.
    fn seal(&mut self) -> Commit<'_> {
        let count = mem::take(&mut self.count);
        assert_ne!(count, 0, "nonempty unconsumed retirement proof");
        Commit {
            identities: &self.identities[..count],
            units: mem::take(&mut self.units),
            bytes: mem::take(&mut self.bytes),
        }
    }
}

impl Commit<'_> {
    fn commit(self, objects: &mut ObjectStorage, batch: &mut SweepBatch<'_>, totals: SweepTotals) {
        batch.objects = self.identities.len();
        batch.units = self.units;
        batch.bytes = self.bytes;
        objects.occupied = totals.live;
        if !objects.initialization_owners.is_empty() {
            for id in self.identities.iter().flatten() {
                objects.initialization_owners.remove(id);
            }
        }
        let sweep = objects.sweep.as_mut().expect("active physical sweep");
        sweep.totals = totals;
        sweep.outstanding = true;
    }
}

impl ObjectStorage {
    /// Capture the old page suffix once. Admissions during Sweep are black;
    /// prepended pages remain outside that captured suffix.
    pub(in crate::gc) fn begin_physical_sweep(
        &mut self,
        graph: &GraphMarker<'_>,
        max_units: usize,
        retained_bytes: u128,
    ) -> Option<()> {
        if graph.phase() != MarkPhase::Sweep || self.sweep.is_some() {
            return None;
        }
        let max_bytes = retained_bytes.checked_sub(self.backing_bytes() as u128)?;
        self.pages.begin_sweep();
        self.sweep = Some(Sweep {
            outstanding: false,
            max_units,
            max_bytes,
            totals: SweepTotals::default(),
        });
        Some(())
    }

    /// Greedily select at most 32 actual owners across physical allocations. All
    /// fragments validate before the first detach. Preview cursors publish only
    /// after successful preflight or successful inspection-only progress.
    pub(in crate::gc) fn sweep_batch(
        &mut self,
        interfaces: &RefCell<InterfaceStore>,
        graph: &mut GraphMarker<'_>,
        budget: &mut ScanBudget,
        batch: &mut SweepBatch<'_>,
    ) -> Option<()> {
        if self.sweep.as_ref()?.outstanding || batch.has_retirements() || batch.fragments != 0 {
            return None;
        }
        let graph = graph.sweep_batch().ok()?;
        let mut prepared = Prepared::new();
        let mut selection = Selection::new();
        let physical = self.pages.prepare_batch(
            &mut selection,
            &mut batch.disposal,
            budget,
            |slot, new_fragment, budget| {
                #[cfg(test)]
                self.discovery_visits.set(self.discovery_visits.get() + 1);
                if graph.is_survivor(slot.mark).ok()? {
                    return Some(Decision::Survivor);
                }
                let reserve = OWNER_RETIRE_WORK
                    + if prepared.count == 0 {
                        BATCH_FINISH_WORK
                    } else {
                        0
                    }
                    + new_fragment.map_or(0, |count| {
                        FRAGMENT_FINISH_WORK + ordering_work(count) - ordering_work(count - 1)
                    });
                // Reserve the monotone proof-bound delta before admitting a fragment.
                if !budget.take(reserve) {
                    return Some(Decision::Pending);
                }
                // Survivors never reconstruct a view or perform retirement charging.
                let charge = slot.payload.view(interfaces)?.charge(interfaces)?;
                prepared.push(
                    slot.payload.direct_reference(),
                    charge.logical_units,
                    charge.retained_bytes,
                )?;
                Some(Decision::Retire)
            },
        )?;
        let sweep = self.sweep.as_ref()?;
        let totals = SweepTotals {
            objects: sweep.totals.objects.checked_add(prepared.count)?,
            units: sweep.totals.units.checked_add(prepared.units)?,
            bytes: sweep.totals.bytes.checked_add(prepared.bytes)?,
            live: self.occupied.checked_sub(prepared.count)?,
        };
        if totals.units > sweep.max_units || totals.bytes > sweep.max_bytes {
            return None;
        }
        let status = physical.status();
        if prepared.count != 0 {
            let credits = graph.prepare_retirement(prepared.count).ok()?;
            let proof = prepared.seal();
            batch.fragments = physical.fragments();
            physical.detach();
            proof.commit(self, batch, totals);
            credits.commit();
        } else {
            physical.detach();
        }
        batch.status = status;
        Some(())
    }

    pub(in crate::gc) fn finish_physical_sweep(&mut self) -> Option<SweepTotals> {
        let sweep = self.sweep.as_ref()?;
        if sweep.outstanding || !self.pages.sweep_done() {
            return None;
        }
        let mut totals = self.sweep.take()?.totals;
        totals.live = self.occupied;
        Some(totals)
    }
}
