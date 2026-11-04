//! Admit activations and carry established state through internal frame transitions.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionFrame, ExecutionStack, NativeEntryState},
    session::ExecutionEvent,
    value::Value,
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
#[derive(Debug, PartialEq)]
pub enum ReturnAction {
    Complete(Value),
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
                .quarantine("native frame resumed before callback completion")),
        }
    }
}

impl ExecutionStack<'_> {
    /// Poll, prepare the diagnostic PC, service GC and notify the observer for
    /// one instruction entry. This shared boundary covers script and Native
    /// activations. Its failures precede the operation's trap-reporting boundary.
    pub fn prepare_instruction_boundary(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        // Poll before changing the diagnostic PC. No user code runs before GC
        // service; the dispatch loop need not repeat this same boundary poll.
        runtime.resources().poll_execution()?;
        self.validate_runtime(runtime)?;
        {
            let mut frames = self
                .session
                .resources
                .sessions
                .frames_mut(self.session.id)
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine("instruction boundary stack is borrowed")
                })?;
            let count = frames.len();
            let frame = frames
                .last_mut()
                .filter(|_| count > self.base)
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine("missing instruction boundary frame")
                })?;
            if matches!(frame.native_entry, NativeEntryState::Script) {
                frame.prepare_instruction();
            }
        }
        runtime.service_gc()?;
        runtime.observe_execution(ExecutionEvent::BeforeInstruction)
    }

    /// Admit initial/resumed activations and classify after an await transition.
    /// Internal calls and returns carry their established action directly.
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
        frame.execution_action(runtime)
    }
}
