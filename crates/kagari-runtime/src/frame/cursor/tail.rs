//! Replacement changes only scalar state; the frame keeps its return/source marks.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    frame::cursor::{ExecutionCursor, kernel::RegionError},
    module::linked_execution::calls::LinkedCall,
};

impl ExecutionCursor<'_, '_, '_, '_> {
    /// None means replacement committed. Otherwise use the exact ordinary call
    /// record; no argument, return continuation or cleanup has been changed.
    pub(super) fn reuse_self_tail(&mut self, index: usize) -> Result<Option<usize>, RegionError> {
        let tail = self
            .function
            .self_tail_calls
            .get(index)
            .ok_or_else(|| self.invalid())?;
        if !self.tail_allowed
            || !self
                .links
                .and_then(|links| links.call_contract(tail.call))
                .is_some_and(LinkedCall::reuses_self)
        {
            return Ok(Some(tail.call));
        }
        // Match ordinary call admission's poll, without committing sticky state
        // or capturing traces until these source/frame loans have ended.
        self.resources.poll_closed()?;
        let call = self
            .function
            .calls
            .get(tail.call)
            .ok_or_else(|| self.invalid())?;
        let frame = &self.function.frame;
        for (source, &home) in call.arguments.iter().zip(&frame.parameter_slots) {
            let value = self
                .values
                .read_location(*source)
                .map_err(RegionError::storage)?;
            if !frame.slot_layout.locations[home].admits(value) {
                return Err(self.invalid());
            }
        }
        let (payloads, initialized) = self.values.scalar_banks().map_err(RegionError::storage)?;
        tail.apply(payloads, initialized, frame)
            .map_err(RegionError::storage)?;
        *self.ip = 0;
        *self.executing = None;
        *self.tail_elided = true;
        #[cfg(feature = "execution-diagnostics")]
        {
            diagnostics::record(Event::TailReplacement);
            diagnostics::record_amount(Event::ArgumentTransfer, call.arguments.len() as u64);
            diagnostics::record_amount(
                Event::BankBytesInitialized,
                frame.slot_layout.scalar_count as u64,
            );
        }
        Ok(None)
    }
}
