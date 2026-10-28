//! Bounded multi-fragment prepare/commit with one aggregate identity/charge proof.
pub(in crate::gc) mod disposal;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transaction_tests;

use self::disposal::SweepBatch;
use super::{
    ObjectStorage, PayloadKind, PayloadLocation,
    descriptors::DescriptorId,
    pages::batch::{Fragment, order::ordering_work},
};
use crate::{
    execution_metadata::interfaces::InterfaceStore,
    gc::{
        HeapObjectId,
        graph::{GraphMarker, MarkPhase, sweep::SweepGraph},
    },
    native::storage::scan::{ScanBudget, ScanStatus},
};
use std::{cell::RefCell, mem};

pub(in crate::gc) const RETIRE_LIMIT: usize = 32;
// Three fixed arrays, output-empty preflight, final empty token-array cleanup,
// and two selection-local bitmap words. Reserve before any initialization,
// including progress-only and failed preparation.
const WORKSPACE_WORK: usize = RETIRE_LIMIT * 5 + 2;
const OWNER_RETIRE_WORK: usize = 8;
// Bitmap checks, physical detach/finish, descriptor counts and final-owner release.
const FRAGMENT_FINISH_WORK: usize = 32;
const BATCH_FINISH_WORK: usize = 32;

enum PreparedOwner {
    Survivor,
    Retire,
    Pending,
}

#[derive(Debug)]
pub(super) struct Sweep {
    pub(super) wrapper: usize,
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

struct Candidate {
    index: usize,
    location: PayloadLocation,
    physical: Option<(DescriptorId, bool)>,
}

struct Prepared {
    owner: u64,
    identities: [Option<HeapObjectId>; RETIRE_LIMIT],
    fragments: [Option<Fragment>; RETIRE_LIMIT],
    fragment_count: usize,
    count: usize,
    units: usize,
    bytes: u128,
}

/// Only borrowed prefixes leave preparation; the large arrays stay in place.
struct Commit<'a> {
    identities: &'a [Option<HeapObjectId>],
    fragments: &'a [Option<Fragment>],
    units: usize,
    bytes: u128,
}

impl Prepared {
    #[inline(always)]
    fn new(owner: u64) -> Self {
        Self {
            owner,
            identities: [None; RETIRE_LIMIT],
            fragments: [None; RETIRE_LIMIT],
            fragment_count: 0,
            count: 0,
            units: 0,
            bytes: 0,
        }
    }

    fn extends(&self, candidate: &Candidate) -> bool {
        candidate.physical.is_some_and(|(descriptor, small)| {
            small
                && self.fragment_count != 0
                && self.fragments[self.fragment_count - 1]
                    .is_some_and(|fragment| fragment.extends(candidate.location, descriptor))
        })
    }

    fn push(
        &mut self,
        candidate: &Candidate,
        generation: u32,
        units: usize,
        bytes: usize,
    ) -> Option<()> {
        self.units = self.units.checked_add(units)?;
        self.bytes = self.bytes.checked_add(bytes as u128)?;
        if let Some((descriptor, small)) = candidate.physical {
            if self.extends(candidate) {
                self.fragments[self.fragment_count - 1].as_mut()?.count += 1;
            } else {
                self.fragments[self.fragment_count] = Some(Fragment {
                    start: candidate.location,
                    descriptor,
                    count: 1,
                    small,
                });
                self.fragment_count += 1;
            }
        }
        self.identities[self.count] =
            Some(HeapObjectId::new(self.owner, candidate.index, generation));
        self.count += 1;
        Some(())
    }

    /// Invalidate before the first mutation. An abandoned or failed page preflight
    /// cannot replay this prefix, and no large workspace is moved into commit.
    fn seal(&mut self) -> Commit<'_> {
        let count = mem::take(&mut self.count);
        assert_ne!(count, 0, "nonempty unconsumed retirement proof");
        let fragments = mem::take(&mut self.fragment_count);
        Commit {
            identities: &self.identities[..count],
            fragments: &self.fragments[..fragments],
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
        let mut free = objects.free_identity;
        for id in self.identities.iter().flatten() {
            let location = objects.slots[id.index()]
                .location()
                .expect("prepared occupied identity");
            if location.kind == PayloadKind::Interface {
                objects.remove_wrapper_prepared(location.page as usize);
            }
            objects.slots[id.index()].retire(id.index() as u32, &mut free);
        }
        objects.free_identity = free;
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
    /// Capture the old page suffix and wrapper extent once. Admissions during
    /// Sweep are black; new pages/wrappers are outside these captured extents.
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
            wrapper: self.interface_directories.len(),
            outstanding: false,
            max_units,
            max_bytes,
            totals: SweepTotals::default(),
        });
        Some(())
    }

    fn prepare_owner(
        &self,
        candidate: &Candidate,
        interfaces: &RefCell<InterfaceStore>,
        graph: &SweepGraph<'_, '_>,
        prepared: &mut Prepared,
        budget: &mut ScanBudget,
    ) -> Option<PreparedOwner> {
        let slot = self.slots.get(candidate.index)?;
        if slot.location()? != candidate.location {
            return None;
        }
        #[cfg(test)]
        self.discovery_visits.set(self.discovery_visits.get() + 1);
        if graph
            .is_survivor(self.mark_for_directory(candidate.index))
            .ok()?
        {
            return Some(PreparedOwner::Survivor);
        }
        let new_fragment = candidate.physical.is_some() && !prepared.extends(candidate);
        let reserve = OWNER_RETIRE_WORK
            + if prepared.count == 0 {
                BATCH_FINISH_WORK
            } else {
                0
            }
            + if new_fragment {
                FRAGMENT_FINISH_WORK + ordering_work(prepared.fragment_count + 1)
                    - ordering_work(prepared.fragment_count)
            } else {
                0
            };
        // Reserve the monotone proof-bound delta before admitting this fragment:
        // both scratch arrays, merge comparisons/writes and adjacent interval checks.
        if !budget.take(reserve) {
            return Some(PreparedOwner::Pending);
        }
        let charge = self
            .get(candidate.location, interfaces)?
            .charge(interfaces)?;
        prepared.push(
            candidate,
            slot.generation,
            charge.logical_units,
            charge.retained_bytes,
        )?;
        Some(PreparedOwner::Retire)
    }

    /// Greedily select at most 32 actual owners across physical allocations. All
    /// fragments validate before the first detach. Preview cursors publish only
    /// after successful preflight or successful inspection-only progress.
    pub(in crate::gc) fn sweep_batch(
        &mut self,
        owner: u64,
        interfaces: &RefCell<InterfaceStore>,
        graph: &mut GraphMarker<'_>,
        budget: &mut ScanBudget,
        batch: &mut SweepBatch<'_>,
    ) -> Option<()> {
        if self.sweep.as_ref()?.outstanding || batch.has_retirements() || batch.fragments != 0 {
            return None;
        }
        let graph = graph.sweep_batch().ok()?;
        let mut prepared = Prepared::new(owner);
        let mut scanner = self.pages.scanner();
        let mut wrapper = self.sweep.as_ref()?.wrapper;
        while prepared.count < RETIRE_LIMIT {
            let Some(slot) = scanner.peek(budget)? else {
                break;
            };
            let candidate = Candidate {
                index: slot.directory as usize,
                location: slot.location,
                physical: Some((slot.descriptor, slot.small)),
            };
            if matches!(
                self.prepare_owner(&candidate, interfaces, &graph, &mut prepared, budget)?,
                PreparedOwner::Pending
            ) {
                break;
            }
            scanner.advance();
        }
        let cursor = scanner.cursor();
        // Never mix page payloads with wrapper swap-removal in a transaction.
        if prepared.count == 0 && cursor.page.is_none() {
            while wrapper != 0 && prepared.count < RETIRE_LIMIT && budget.take(1) {
                let ordinal = wrapper - 1;
                let candidate = Candidate {
                    index: self.interface_directories[ordinal].directory as usize,
                    location: PayloadLocation {
                        kind: PayloadKind::Interface,
                        page: ordinal as u32,
                        slot: 0,
                    },
                    physical: None,
                };
                if matches!(
                    self.prepare_owner(&candidate, interfaces, &graph, &mut prepared, budget)?,
                    PreparedOwner::Pending
                ) {
                    break;
                }
                wrapper -= 1;
            }
        }
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
        let status = if cursor.page.is_none() && wrapper == 0 {
            ScanStatus::Complete
        } else {
            ScanStatus::Pending
        };
        if prepared.count != 0 {
            if prepared.fragment_count == 0 {
                self.prepare_wrapper_removal(prepared.count)?;
            }
            let credits = graph.prepare_retirement(prepared.count).ok()?;
            let proof = prepared.seal();
            let physical = self
                .pages
                .prepare_batch(proof.fragments, &mut batch.disposal)?;
            batch.fragments = proof.fragments.len();
            physical.detach();
            self.pages.publish_sweep(cursor);
            self.sweep.as_mut()?.wrapper = wrapper;
            proof.commit(self, batch, totals);
            credits.commit();
        } else {
            self.pages.publish_sweep(cursor);
            self.sweep.as_mut()?.wrapper = wrapper;
        }
        batch.status = status;
        Some(())
    }

    pub(in crate::gc) fn finish_physical_sweep(&mut self) -> Option<SweepTotals> {
        let sweep = self.sweep.as_ref()?;
        if sweep.outstanding || !self.pages.sweep_done() || sweep.wrapper != 0 {
            return None;
        }
        let mut totals = self.sweep.take()?.totals;
        totals.live = self.occupied;
        Some(totals)
    }
}
