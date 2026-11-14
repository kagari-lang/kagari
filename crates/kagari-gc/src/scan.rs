//! Cooperative work accounting and the only visitor for managed graph edges.

use crate::{
    allocation::PhysicalHeap,
    error::GcError,
    marking::Marking,
    object::{Edge, ObjectRef},
};

/// A representation's declared tracing bound; units include inspected holes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanKind {
    EdgeFree,
    Bounded { max_work: usize },
    Resumable,
    Indivisible,
}

/// Whether the captured finite extent has been fully inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    Pending,
    Complete,
}

/// A service allowance. One unit is logical work, not a time guarantee.
#[derive(Debug, Clone, Copy)]
pub struct WorkBudget(pub(crate) usize);

impl WorkBudget {
    pub const NORMAL: Self = Self(1024);

    pub const fn new(units: usize) -> Self {
        Self(units)
    }
}

/// Shared allowance for provider inspection and marker bitmap initialization.
#[derive(Debug)]
pub struct ScanBudget {
    remaining: usize,
    spent: usize,
}

impl ScanBudget {
    pub fn new(units: usize) -> Self {
        Self {
            remaining: units,
            spent: 0,
        }
    }

    /// Reserve before advancing state; false does not change either counter.
    pub fn take(&mut self, units: usize) -> bool {
        if units > self.remaining || self.spent.checked_add(units).is_none() {
            return false;
        }
        self.remaining -= units;
        self.spent += units;
        true
    }

    /// Charge an explicitly indivisible operation, even beyond this allowance.
    pub fn charge(&mut self, units: usize) -> Result<(), GcError> {
        self.spent = self
            .spent
            .checked_add(units)
            .ok_or(GcError::Limit("scan work"))?;
        self.remaining = self.remaining.saturating_sub(units);
        Ok(())
    }

    pub fn spent(&self) -> usize {
        self.spent
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

/// Visits edges while their admitted object remains borrowed and live.
pub struct EdgeVisitor<'a> {
    pub(crate) physical: &'a PhysicalHeap,
    pub(crate) target: VisitTarget<'a>,
}

pub(crate) enum VisitTarget<'a> {
    Mark(&'a mut Marking),
    Inspect {
        visit: &'a mut dyn FnMut(ObjectRef) -> bool,
        indivisible: bool,
    },
}

impl EdgeVisitor<'_> {
    /// Protect a stored target before its source scan advances.
    ///
    /// # Safety
    /// The edge belongs to the current admitted same-heap object, driver root source or traced scratch;
    /// its source stays live through this call. An arbitrary escaped carrier is
    /// insufficient. Pending leaves the edge unconsumed so the provider retries it.
    pub unsafe fn try_visit_stored<T: 'static>(
        &mut self,
        edge: &Edge<T>,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, GcError> {
        // SAFETY: inherited admitted-source contract.
        unsafe { self.try_visit_reference(edge.reference, budget) }
    }

    /// The caller holds an initialized stored reference in a live source object.
    pub(crate) unsafe fn try_visit_reference(
        &mut self,
        reference: ObjectRef,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, GcError> {
        match &mut self.target {
            VisitTarget::Mark(marking) => marking.publish(self.physical, reference, Some(budget)),
            VisitTarget::Inspect { visit, indivisible } => {
                if *indivisible {
                    budget.charge(1)?;
                } else if !budget.take(1) {
                    return Ok(ScanStatus::Pending);
                }
                self.physical.resolve(reference)?;
                Ok(if visit(reference) {
                    ScanStatus::Complete
                } else {
                    ScanStatus::Pending
                })
            }
        }
    }
}
