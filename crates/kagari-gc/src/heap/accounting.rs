//! Readonly interval observations derived from the committed heap ledger.
use crate::{allocation::PhysicalHeap, error::GcError};

/// Committed logical occupancy and provider-owned indirect capacity.
///
/// Units include one per managed object plus its declared provider units. Bytes
/// exclude physical heap backing, descriptor/control storage and Host resources.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Occupancy {
    pub units: u128,
    pub indirect_bytes: usize,
}

/// One closed access or collector-service interval, including error/unwind work.
///
/// Peaks observe actual allocation/mutation commits, including growth followed by
/// shrinkage in the same interval. Independent unit/byte maxima need not occur
/// at the same instant. A failed preflight contributes no uncommitted request.
/// This detached report grants no heap access and has no session or quota policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AccountingReport {
    pub before: Occupancy,
    pub after: Occupancy,
    pub peak: Occupancy,
}

#[derive(Debug, Default)]
pub(crate) struct MemoryLedger {
    pub(crate) indirect: usize,
    pub(crate) units: u128,
    pub(crate) peak: usize,
    pub(crate) allocation_growth: usize,
    before: Occupancy,
    peak_interval: Occupancy,
}

impl MemoryLedger {
    fn occupancy(&self, physical: &PhysicalHeap) -> Occupancy {
        // Each of at most usize::MAX objects contributes at most usize::MAX
        // provider units plus one wrapper; the combined sum fits in u128.
        Occupancy {
            units: self.units + physical.objects as u128,
            indirect_bytes: self.indirect,
        }
    }

    pub(crate) fn begin(&mut self, physical: &PhysicalHeap) {
        self.before = self.occupancy(physical);
        self.peak_interval = self.before;
    }

    pub(crate) fn report(&self, physical: &PhysicalHeap) -> AccountingReport {
        AccountingReport {
            before: self.before,
            after: self.occupancy(physical),
            peak: self.peak_interval,
        }
    }

    pub(crate) fn check_units(&self, old: usize, new: usize) -> Result<u128, GcError> {
        self.units
            .checked_sub(old as u128)
            .and_then(|units| units.checked_add(new as u128))
            .ok_or(GcError::Storage("provider unit accounting"))
    }

    pub(crate) fn grew(&mut self, bytes: usize) {
        self.allocation_growth = self.allocation_growth.saturating_add(bytes);
    }

    pub(crate) fn check(&self, old: usize, new: usize, limit: usize) -> Result<usize, GcError> {
        let total = self
            .indirect
            .checked_sub(old)
            .and_then(|bytes| bytes.checked_add(new))
            .ok_or(GcError::Limit("indirect accounting"))?;
        if total > limit {
            return Err(GcError::Limit("indirect bytes"));
        }
        Ok(total)
    }

    pub(crate) fn observe(&mut self, physical: &PhysicalHeap) {
        let current = self.occupancy(physical);
        self.peak_interval.units = self.peak_interval.units.max(current.units);
        self.peak_interval.indirect_bytes = self
            .peak_interval
            .indirect_bytes
            .max(current.indirect_bytes);
        self.peak = self
            .peak
            .max(physical.backing_bytes.saturating_add(self.indirect));
    }
}
