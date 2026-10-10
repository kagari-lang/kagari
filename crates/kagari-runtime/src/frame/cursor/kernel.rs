//! Closed operations reuse authority without allocating or admitting callbacks.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    error::RuntimeError,
    frame::{
        cursor::{ExecutionCursor, scalars::ScalarExit},
        transfer::ReturnValue,
    },
    module::execution::ExecutionInstruction,
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
    pub fn execute_region(
        &mut self,
        remaining: &mut Option<usize>,
    ) -> Result<RegionExit, RuntimeError> {
        self.runtime
            .resources()
            .ensure_cursor_allowed(&self.session)?;
        self.runtime.gc().ensure_no_native_borrow()?;
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
                ScalarExit::Region(exit) => {
                    #[cfg(feature = "execution-diagnostics")]
                    if matches!(exit, RegionExit::Boundary) {
                        diagnostics::record(Event::SlowBoundary);
                    }
                    return Ok(exit);
                }
                ScalarExit::Object(ExecutionInstruction::ReadField { dst, base, field }) => {
                    if !self.read_field(dst, base, field)? {
                        #[cfg(feature = "execution-diagnostics")]
                        diagnostics::record(Event::SlowBoundary);
                        return Ok(RegionExit::Boundary);
                    }
                }
                ScalarExit::Object(ExecutionInstruction::WriteField { base, value, field }) => {
                    if !self.write_field(base, value, field)? {
                        #[cfg(feature = "execution-diagnostics")]
                        diagnostics::record(Event::SlowBoundary);
                        return Ok(RegionExit::Boundary);
                    }
                }
                ScalarExit::Object(_) => unreachable!("sealed object operation"),
            }
            // The field's PC and slice unit were consumed before the handoff.
            // Its successor still needs the normal logical boundary checks.
            first = false;
        }
    }
}
