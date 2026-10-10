//! Only an installed runtime-owned body can execute without the native callback boundary.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::frame::cursor::{
    ExecutionCursor,
    kernel::{CursorExit, RegionError, RegionExit},
};

impl ExecutionCursor<'_> {
    #[inline(never)]
    pub(super) fn execute_native(
        &mut self,
        index: usize,
    ) -> Result<Option<CursorExit>, RegionError> {
        let operation = self
            .frame
            .links
            .as_ref()
            .and_then(|links| links.primitive(index))
            .ok_or_else(|| self.invalid())?;
        let Some(operation) = operation else {
            // Arbitrary Rust bodies and result adapters retain ordinary native
            // invocation. This decision comes from the linked implementation,
            // not the method name, argument representation or benchmark source.
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::record(Event::SlowBoundary);
            return Ok(Some(CursorExit::Region(RegionExit::Boundary)));
        };
        // Preserve both native cancellation polls, including post-body failure
        // precedence. The bounded kernel cannot allocate script objects or reenter.
        self.runtime.resources().poll_execution()?;
        let source = self
            .values
            .read_location(operation.source)
            .ok_or_else(|| self.invalid())?;
        let result = operation.operation.execute(self.runtime.gc(), source);
        self.runtime.resources().poll_execution()?;
        let value = result?;
        if let Some(destination) = operation.destination {
            if !self.runtime.gc().validate_value(&value) {
                return Err(self.invalid().into());
            }
            self.values
                .write_location(destination, value)
                .ok_or_else(|| self.invalid())?;
        }
        Ok(None)
    }
}
