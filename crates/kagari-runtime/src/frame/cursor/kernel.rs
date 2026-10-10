//! Closed operations reuse authority without allocating or admitting callbacks.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    error::RuntimeError,
    frame::{
        cursor::{ExecutionCursor, scalars::ScalarExit},
        transfer::ReturnValue,
    },
    module::execution::managed::{ManagedOperation, PreparedManagedOperation},
};

pub enum RegionExit {
    Slice,
    Safepoint,
    Boundary,
    Return(ReturnValue),
}

impl ExecutionCursor<'_> {
    /// Execute sealed nonallocating operations under one authority check. The driver
    /// has already polled and observed the first PC; subsequent logical PCs keep
    /// the same cancellation, collection and observation boundaries. No caller
    /// supplies values or callbacks while authority is reused.
    pub(super) fn execute_region(
        &mut self,
        remaining: &mut Option<usize>,
    ) -> Result<RegionExit, RuntimeError> {
        // A closed region cannot allocate heap records, mutate executable metadata
        // or change collector policy. Field operations copy rooted Values using
        // checked storage methods; no destructor/callback runs on replacement.
        // Recompute after every allocating or reentrant boundary.
        // Abandoned program leases can expire on another thread and remain
        // checked at each logical PC, as do cancellation and observer requests.
        let collection_due = self.runtime.gc().collection_due();
        let mut first = true;
        loop {
            match self.scalars()?.execute(remaining, collection_due, first)? {
                ScalarExit::Slice => return Ok(RegionExit::Slice),
                ScalarExit::Safepoint => return Ok(RegionExit::Safepoint),
                ScalarExit::Return(value) => return Ok(RegionExit::Return(value)),
                ScalarExit::Boundary => {
                    #[cfg(feature = "execution-diagnostics")]
                    diagnostics::record(Event::SlowBoundary);
                    return Ok(RegionExit::Boundary);
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Copy {
                    dst,
                    src,
                })) => {
                    // The admitted banks own both roots. Keep the ordinary heap
                    // identity and destination representation checks; copying a
                    // Value cannot allocate, collect, run a destructor or reenter.
                    let value = self
                        .values
                        .read_location(src)
                        .ok_or_else(|| self.invalid())?;
                    if !self.runtime.gc().validate_value(&value) {
                        return Err(self.invalid());
                    }
                    self.values
                        .write_location(dst, value)
                        .ok_or_else(|| self.invalid())?;
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Return {
                    value,
                })) => {
                    // The callee window retains this value through adaptation.
                    // Common retirement/publication permits no GC or callback gap.
                    let value = self
                        .values
                        .managed
                        .get(value.index())
                        .copied()
                        .ok_or_else(|| self.invalid())?;
                    return Ok(RegionExit::Return(ReturnValue::general(value)));
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::ReadField {
                    dst,
                    base,
                    field,
                })) => {
                    if !self.read_field(dst, base, field)? {
                        #[cfg(feature = "execution-diagnostics")]
                        diagnostics::record(Event::SlowBoundary);
                        return Ok(RegionExit::Boundary);
                    }
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::WriteField {
                    base,
                    value,
                    field,
                })) => {
                    if !self.write_field(base, value, field)? {
                        #[cfg(feature = "execution-diagnostics")]
                        diagnostics::record(Event::SlowBoundary);
                        return Ok(RegionExit::Boundary);
                    }
                }
            }
            // The operation's PC and slice unit were consumed before the handoff.
            // Its successor still needs the normal logical boundary checks.
            first = false;
        }
    }
}
