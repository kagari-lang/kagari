//! Classify authoritative runtime state at activation and frame/wait transitions.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, NativeEntryState},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionAction {
    Script,
    Await,
    NativeEntry,
    NativeReturn,
}

impl ExecutionStack<'_> {
    /// Ordinary instructions retain their current action. Call/return/await and
    /// resumed activations classify again after transient borrows have ended.
    pub fn next_action(&self, runtime: &Runtime) -> Result<ExecutionAction, RuntimeError> {
        self.validate_runtime(runtime)?;
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::DriverAdmission);
        let state = self.session.state();
        if state.queued_factory.borrow().is_some()
            || state.queued_future.borrow().is_some()
            || state.pending.borrow().is_some()
        {
            return Ok(ExecutionAction::Await);
        }
        let frames = self.frames()?;
        let frame = frames
            .last()
            .filter(|_| frames.len() > self.base)
            .ok_or_else(|| runtime.resources().quarantine("missing execution frame"))?;
        match frame.native_entry {
            NativeEntryState::Script => Ok(ExecutionAction::Script),
            NativeEntryState::Pending => Ok(ExecutionAction::NativeEntry),
            NativeEntryState::Complete => Ok(ExecutionAction::NativeReturn),
            NativeEntryState::Running => Err(runtime
                .resources()
                .quarantine("native frame resumed before callback completion")),
        }
    }
}
