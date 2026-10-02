use kagari_common::cancellation::CancellationToken;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    session::{ExecutionEvent, ExecutionObserver},
};
use std::cell::Cell;

/// Test-only interruption at an observed execution boundary; not an engine budget.
#[derive(Debug)]
pub struct CancelAt {
    pub seen: Cell<usize>,
    pub at: usize,
    pub token: CancellationToken,
}
impl ExecutionObserver for CancelAt {
    fn observe(
        &self,
        runtime: &Runtime,
        event: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        if event == ExecutionEvent::BeforeInstruction {
            let seen = self.seen.get();
            self.seen.set(seen + 1);
            if seen == self.at {
                self.token.cancel();
            }
        }
        runtime.resources().poll_execution()
    }
}
