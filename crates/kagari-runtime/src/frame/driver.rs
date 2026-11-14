//! Admit activations and carry established state through internal frame transitions.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::{ExecutionFrame, FrameStack, NativeEntryState, transfer::ReturnValue},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionAction {
    Script,
    Await,
    NativeEntry,
    NativeReturn,
}

/// A completed return either exits this stack scope or resumes an established state.
/// The driver must still poll cancellation, observe instructions and honor slices.
/// Continuations belong to the current activation; a later resume admits again.
#[derive(Debug)]
pub enum ReturnAction {
    Complete(ReturnValue),
    Continue(ExecutionAction),
}

impl ExecutionFrame {
    pub(super) fn execution_action(
        &self,
        runtime: &Runtime,
    ) -> Result<ExecutionAction, RuntimeError> {
        match self.native_entry {
            NativeEntryState::Script => Ok(ExecutionAction::Script),
            NativeEntryState::Pending => Ok(ExecutionAction::NativeEntry),
            NativeEntryState::Complete => Ok(ExecutionAction::NativeReturn),
            NativeEntryState::Running => Err(runtime
                .resources()
                .quarantine(EngineFault::NativeFrameResumedBeforeCallbackCompletion)),
        }
    }
}

impl FrameStack {
    /// Admit initial/resumed activations and classify after an await transition.
    /// Internal calls and returns carry their established action directly.
    pub fn next_action(&self, runtime: &Runtime) -> Result<ExecutionAction, RuntimeError> {
        self.validate_top(runtime)?;
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::DriverAdmission);
        let state = self.session_state(runtime);
        if state.queued_factory.borrow().is_some()
            || state.queued_future.borrow().is_some()
            || state.pending.borrow().is_some()
        {
            return Ok(ExecutionAction::Await);
        }
        let frames = self.frames(runtime)?;
        let frame = frames
            .last()
            .filter(|_| frames.len() > self.base)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::MissingExecutionFrame)
            })?;
        frame.execution_action(runtime)
    }
}
