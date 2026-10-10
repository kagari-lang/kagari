//! An admitted interpreter scope cannot escape the closed execution operation.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        ExecutionFrame, ExecutionStack,
        cursor::kernel::{CursorExit, RegionError, RegionExit},
        values::operands::OperandWindow,
    },
    session::SessionState,
};
use std::ptr;

mod indices;
pub mod kernel;
mod objects;
mod scalars;
#[cfg(test)]
mod tests;
mod transitions;

/// All references live inside execute_region. No caller callback or supplied Value
/// can intervene while admission is reused; exits release the frame and bank borrows.
struct ExecutionCursor<'a> {
    frame: &'a mut ExecutionFrame,
    values: OperandWindow<'a>,
    runtime: &'a Runtime,
    session: &'a SessionState,
}

impl ExecutionStack<'_> {
    /// Run a closed region, releasing all transient views before returning a transition.
    /// The driver has already polled and observed the first logical instruction.
    pub fn execute_region(
        &self,
        runtime: &Runtime,
        remaining: &mut Option<usize>,
    ) -> Result<RegionExit, RegionError> {
        if !ptr::eq(runtime.resources(), self.session.resources) {
            return Err(RuntimeError::module_validation(
                "execution stack belongs to another runtime",
            )
            .into());
        }
        match self.execute_admitted_region(runtime, remaining)? {
            CursorExit::Region(exit) => Ok(exit),
            CursorExit::Transition(transition) => self.complete_transition(runtime, transition),
        }
    }

    fn execute_admitted_region(
        &self,
        runtime: &Runtime,
        remaining: &mut Option<usize>,
    ) -> Result<CursorExit, RegionError> {
        // current_mut admits the active session/frame scope and sticky termination.
        let mut frame = self.current_mut()?;
        runtime.gc().ensure_no_native_borrow()?;
        let mut values = runtime
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| {
                runtime
                    .resources()
                    .quarantine("execution slots borrowed across instruction")
            })?;
        let values = values.borrow_operands(frame.slots).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid execution frame window")
        })?;
        let session = self.session.state();
        ExecutionCursor {
            frame: &mut frame,
            values,
            runtime,
            session: &session,
        }
        .execute_region(remaining)
    }
}

impl ExecutionCursor<'_> {
    fn invalid(&self) -> RuntimeError {
        self.runtime
            .resources()
            .quarantine("invalid execution operand slot")
    }
}
