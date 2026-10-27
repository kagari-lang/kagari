//! One bounded prepare/commit owner for physical sweep and committed charges.
#[cfg(test)]
mod tests;

use super::{
    ObjectStorage, PayloadKind, PayloadLocation,
    pages::{Retired, retirement::RetiredRange},
};
use crate::{
    execution_metadata::interfaces::InterfaceStore,
    gc::{
        GcHeap, HeapObjectId,
        graph::{GraphMarker, MarkPhase, sweep::SweepGraph},
    },
    native::storage::scan::ScanBudget,
};
use std::cell::RefCell;

pub(in crate::gc) const RETIRE_LIMIT: usize = 32;

#[derive(Debug)]
pub(super) struct Sweep {
    wrapper: usize,
    wrapper_end: usize,
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

enum Disposal {
    Payload(Retired),
    Range(RetiredRange),
}

/// Fixed workspace, not an allocation proportional to the dead population. This
/// proof never escapes prepare/commit's exclusive, noncallback storage borrow.
struct Prepared {
    identities: [Option<HeapObjectId>; RETIRE_LIMIT],
    start: Option<PayloadLocation>,
    count: usize,
    units: usize,
    bytes: u128,
}

impl Prepared {
    fn new() -> Self {
        Self {
            identities: [None; RETIRE_LIMIT],
            start: None,
            count: 0,
            units: 0,
            bytes: 0,
        }
    }

    fn push(
        &mut self,
        id: HeapObjectId,
        location: PayloadLocation,
        units: usize,
        bytes: usize,
    ) -> Option<()> {
        self.units = self.units.checked_add(units)?;
        self.bytes = self.bytes.checked_add(bytes as u128)?;
        self.start.get_or_insert(location);
        self.identities[self.count] = Some(id);
        self.count += 1;
        Some(())
    }

    /// Consume the checked identities without repeating batch-wide occupancy or
    /// free-list setup. No callback or admission can intervene after preparation.
    fn commit(
        self,
        objects: &mut ObjectStorage,
        disposal: Option<Disposal>,
        totals: SweepTotals,
        complete: bool,
    ) -> SweepBatch {
        let identities = &self.identities[..self.count];
        let mut free = objects.free_identity;
        for id in identities.iter().flatten() {
            objects.slots[id.index()].retire(id.index() as u32, &mut free);
        }
        objects.free_identity = free;
        objects.occupied = totals.live;
        // Consult the actual owner once, not a shadow initialization counter.
        if !objects.initialization_owners.is_empty() {
            for id in identities.iter().flatten() {
                objects.initialization_owners.remove(id);
            }
        }
        let sweep = objects.sweep.as_mut().expect("active physical sweep");
        sweep.totals = totals;
        sweep.outstanding = true;
        SweepBatch {
            disposal,
            units: self.units,
            bytes: self.bytes,
            complete,
        }
    }
}

pub(in crate::gc) struct SweepBatch {
    disposal: Option<Disposal>,
    units: usize,
    bytes: u128,
    pub(in crate::gc) complete: bool,
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
        // Backing is accounted separately at final release. Reserve its entire
        // committed total, including survivor pages, before accepting payload sums.
        let max_bytes = retained_bytes.checked_sub(self.backing_bytes() as u128)?;
        self.pages.begin_sweep();
        self.sweep = Some(Sweep {
            wrapper: 0,
            wrapper_end: self.interface_directories.len(),
            outstanding: false,
            max_units,
            max_bytes,
            totals: SweepTotals::default(),
        });
        Some(())
    }

    fn prepare_owner(
        &self,
        owner: u64,
        index: usize,
        location: PayloadLocation,
        interfaces: &RefCell<InterfaceStore>,
        graph: &SweepGraph<'_, '_>,
        prepared: &mut Prepared,
    ) -> Option<bool> {
        let slot = self.slots.get(index)?;
        if slot.location()? != location {
            return None;
        }
        #[cfg(test)]
        self.discovery_visits.set(self.discovery_visits.get() + 1);
        let mark = self.mark_for_directory(index);
        if graph.is_survivor(mark).ok()? {
            return Some(true);
        }

        let charge = self.get(location, interfaces)?.charge(interfaces)?;
        prepared.push(
            HeapObjectId::new(owner, index, slot.generation),
            location,
            charge.logical_units,
            charge.retained_bytes,
        )?;
        Some(false)
    }

    /// Each call returns at most one page-local descriptor range (or one large
    /// owner), never splitting a small page just to fill another page's batch.
    /// Every inspected slot, including holes and survivors, consumes budget.
    pub(in crate::gc) fn sweep_batch(
        &mut self,
        owner: u64,
        interfaces: &RefCell<InterfaceStore>,
        graph: &mut GraphMarker<'_>,
        budget: &mut ScanBudget,
    ) -> Option<SweepBatch> {
        if self.sweep.as_ref()?.outstanding {
            return None;
        }
        let graph = graph.sweep_batch().ok()?;
        let mut prepared = Prepared::new();
        let mut descriptor = None;

        let mut small = false;
        let page = self.pages.sweep_page();
        while page.is_some()
            && self.pages.sweep_page() == page
            && prepared.count < RETIRE_LIMIT
            && budget.take(1)
        {
            let candidate = self.pages.sweep_slot();
            if prepared.count != 0
                && !candidate.as_ref().is_some_and(|slot| {
                    Some(slot.descriptor) == descriptor
                        && slot.location.page == prepared.start.unwrap().page
                        && slot.location.slot as usize
                            == prepared.start.unwrap().slot as usize + prepared.count
                })
            {
                break;
            }
            if let Some(slot) = candidate {
                small = slot.small;
                descriptor = Some(slot.descriptor);
                let survivor = self.prepare_owner(
                    owner,
                    slot.directory as usize,
                    slot.location,
                    interfaces,
                    &graph,
                    &mut prepared,
                )?;

                self.pages.advance_sweep();
                if !small || (survivor && prepared.count != 0) {
                    break;
                }
            } else {
                self.pages.advance_sweep();
            }
        }
        if prepared.count == 0 && self.pages.sweep_done() {
            while prepared.count < RETIRE_LIMIT {
                let sweep = self.sweep.as_mut()?;
                if sweep.wrapper == sweep.wrapper_end || !budget.take(1) {
                    break;
                }
                let ordinal = sweep.wrapper;
                sweep.wrapper += 1;
                let wrapper = &self.interface_directories[ordinal];
                let location = PayloadLocation {
                    kind: PayloadKind::Interface,
                    page: ordinal as u32,
                    slot: 0,
                };
                // A prior take/retirement may have reused the directory. The old
                // wrapper ordinal must not name its replacement's metadata.
                if self.slots[wrapper.directory as usize].location() != Some(location) {
                    continue;
                }
                self.prepare_owner(
                    owner,
                    wrapper.directory as usize,
                    location,
                    interfaces,
                    &graph,
                    &mut prepared,
                )?;
            }
        }
        let sweep = self.sweep.as_mut()?;
        let totals = SweepTotals {
            objects: sweep.totals.objects.checked_add(prepared.count)?,
            units: sweep.totals.units.checked_add(prepared.units)?,
            bytes: sweep.totals.bytes.checked_add(prepared.bytes)?,
            live: self.occupied.checked_sub(prepared.count)?,
        };
        if totals.units > sweep.max_units || totals.bytes > sweep.max_bytes {
            return None;
        }
        let complete = self.pages.sweep_done() && sweep.wrapper == sweep.wrapper_end;
        let credits = graph.prepare_retirement(prepared.count).ok()?;
        // All fallible charge/identity checks precede detachment. A range validates
        // its complete initialized interval before mutating any slot; no callbacks
        // or allocations occur between preparation and consuming these identities.
        let disposal = if let Some(start) = prepared.start {
            if start.kind == PayloadKind::Interface {
                None
            } else if small {
                Some(Disposal::Range(
                    self.pages.prepare_range(start, prepared.count)?.detach(),
                ))
            } else {
                Some(Disposal::Payload(self.pages.detach(start)?))
            }
        } else {
            None
        };
        let batch = prepared.commit(self, disposal, totals, complete);
        credits.commit();
        Some(batch)
    }

    pub(in crate::gc) fn finish_physical_sweep(&mut self) -> Option<SweepTotals> {
        let sweep = self.sweep.as_ref()?;
        if sweep.outstanding || !self.pages.sweep_done() || sweep.wrapper != sweep.wrapper_end {
            return None;
        }
        let mut totals = self.sweep.take()?.totals;
        // A take or newborn between slices can change an already visited page.
        totals.live = self.occupied;
        Some(totals)
    }
}

impl GcHeap {
    /// Accounting remains committed during payload destruction and final dispatch /
    /// backing-owner release. A panic quarantines before the next destructor runs.
    pub(in crate::gc) fn dispose_sweep_batch(&self, mut batch: SweepBatch) -> usize {
        match &mut batch.disposal {
            Some(Disposal::Payload(payload)) => {
                if payload.dispose() {
                    self.resources
                        .quarantine("native payload destruction panicked");
                }
            }
            Some(Disposal::Range(range)) => {
                range.dispose(|| {
                    self.resources
                        .quarantine("native payload destruction panicked");
                });
            }
            None => {}
        }
        let (released, disposal) = {
            let mut objects = self.objects.borrow_mut();
            match batch.disposal {
                Some(Disposal::Payload(payload)) => {
                    let (bytes, payload) = objects.finish(payload);
                    (bytes, Some(Disposal::Payload(payload)))
                }
                Some(Disposal::Range(range)) => (
                    objects.pages.finish_range(&range),
                    Some(Disposal::Range(range)),
                ),
                None => (0, None),
            }
        };
        let panicked = match disposal {
            Some(Disposal::Payload(payload)) => payload.release(),
            Some(Disposal::Range(range)) => range.release(),
            None => false,
        };
        if panicked {
            self.resources
                .quarantine("representation owner destruction panicked");
        }
        self.resources.release_heap_totals(
            batch.units,
            batch
                .bytes
                .checked_add(released as u128)
                .expect("prepared payload and backing bound"),
        );
        self.objects
            .borrow_mut()
            .sweep
            .as_mut()
            .expect("active physical sweep")
            .outstanding = false;
        released
    }
}
