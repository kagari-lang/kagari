//! Sealed Native bodies execute only inside the admitted collection-free scope.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    error::{RuntimeError, reason::CancellationReason},
    frame::cursor::{ExecutionCursor, kernel::RegionError},
    module::linked_execution::LinkedNativeBody,
    native::primitive::string_byte_length,
};
use kagari_gc::error::GcError;

pub(super) enum NativeContinuation {
    Complete,
    Callback,
}

impl ExecutionCursor<'_, '_, '_, '_> {
    #[inline(never)]
    pub(super) fn execute_native(
        &mut self,
        index: usize,
    ) -> Result<NativeContinuation, RegionError> {
        let operation = self
            .links
            .and_then(|links| links.native_call(index))
            .ok_or_else(|| self.invalid())?;
        let source = match &operation.body {
            LinkedNativeBody::Callback(_) => {
                #[cfg(feature = "execution-diagnostics")]
                diagnostics::record(Event::SlowBoundary);
                return Ok(NativeContinuation::Callback);
            }
            LinkedNativeBody::StringByteLength(source) => *source,
        };
        let destination = operation.destination;
        self.poll_native()?;
        let result = self
            .values
            .read_location(source)
            .map_err(RegionError::storage)
            .map(|value| string_byte_length(self.values.scope(), value));
        // Check after the body even on failure, before unwrapping its outcome or
        // changing caller storage. Trace/sticky termination is admitted only
        // after the cursor has released all frame/source/session loans.
        self.poll_native()?;
        let value = result?.map_err(|error| match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => RuntimeError::module_validation(error.to_string()).into(),
            error => RegionError::storage(error),
        })?;
        if let Some(destination) = destination {
            self.values
                .write_location(destination, value)
                .map_err(RegionError::storage)?;
        }
        Ok(NativeContinuation::Complete)
    }

    fn poll_native(&self) -> Result<(), RegionError> {
        // The closed region cannot change execution phase or session selection.
        // Reuse its admitted session without querying the control directory.
        if let Some(error) = self.session.termination.borrow().as_ref() {
            return Err(error.clone().into());
        }
        if self.session.options.cancellation.check().is_err() {
            return Err(RuntimeError::from(CancellationReason::ExecutionCancelled).into());
        }
        Ok(())
    }
}
