//! Ordinary progress retains its action; frame and wait transitions re-enter runtime state.
use crate::{
    error::VmError,
    executor::{DriveOutcome, Executor, dispatch::InstructionProgress, native},
};
use kagari_bytecode::module::CallableTarget;
use kagari_runtime::{
    frame::{cursor::kernel::RegionExit, driver::ExecutionAction},
    session::ExecutionEvent,
};
use std::task::Poll;

impl Executor<'_> {
    pub(super) fn run_inner(
        &mut self,
        mut remaining: Option<usize>,
    ) -> Result<DriveOutcome, VmError> {
        // None denotes a real activation/frame/wait transition, never an ordinary PC.
        let mut action = None;
        loop {
            self.runtime.resources().poll_execution()?;
            if action.is_none() {
                action = Some(self.stack.next_action(self.runtime)?);
            }
            if action == Some(ExecutionAction::Await) {
                let resumed = self
                    .stack
                    .poll_await(self.runtime)
                    .map_err(VmError::RuntimeError);
                match self.report_operation(resumed)? {
                    Poll::Pending => return Ok(DriveOutcome::Waiting),
                    Poll::Ready(Some(value)) => return Ok(DriveOutcome::Complete(value)),
                    Poll::Ready(None) => {
                        action = Some(self.stack.next_action(self.runtime)?);
                    }
                }
            }
            // A completed native result remains rooted in its frame if slicing parks here.
            if remaining == Some(0) && self.stack.can_park(self.runtime)? {
                return Ok(DriveOutcome::Runnable);
            }
            match action.expect("classified activation") {
                ExecutionAction::NativeReturn => {
                    let result = self
                        .stack
                        .finish_native_return(self.runtime)
                        .map_err(VmError::RuntimeError);
                    if let Some(value) = self.report_operation(result)? {
                        return Ok(DriveOutcome::Complete(value));
                    }
                    action = None;
                    continue;
                }
                ExecutionAction::NativeEntry => {
                    if let Some(remaining) = &mut remaining {
                        *remaining = remaining.saturating_sub(1);
                    }
                    self.runtime.gc_safepoint()?;
                    self.runtime
                        .observe_execution(ExecutionEvent::BeforeInstruction)?;
                    let result = self
                        .stack
                        .start_native_entry(self.runtime, native::invoke_script)
                        .map_err(VmError::RuntimeError);
                    action = Some(self.report_operation(result)?);
                    continue;
                }
                ExecutionAction::Script => {}
                ExecutionAction::Await => {
                    return Err(VmError::RuntimeError(
                        self.runtime.quarantine_execution_invariant(
                            "ready wait retained a pending activation",
                        ),
                    ));
                }
            }
            self.current_frame_mut()?.prepare_instruction();
            self.runtime.gc_safepoint().map_err(VmError::RuntimeError)?;
            self.runtime
                .observe_execution(ExecutionEvent::BeforeInstruction)?;

            let result = self
                .stack
                .execute_region(self.runtime, &mut remaining)
                .map_err(VmError::from);
            let progress = match self.report_operation(result)? {
                RegionExit::Safepoint | RegionExit::Slice => continue,
                RegionExit::Return(value) => InstructionProgress::Return(value),
                RegionExit::Boundary => {
                    let (loaded, target, pc) = {
                        let frame = self.current_frame()?;
                        (
                            frame.loaded().clone(),
                            frame.target(),
                            frame.instruction_offset(),
                        )
                    };
                    let CallableTarget::Script(function) = target else {
                        return Err(VmError::UnsupportedInstruction(
                            "native boundary in script cursor",
                        ));
                    };
                    let instruction = loaded
                        .bytecode
                        .functions
                        .get(function.index())
                        .and_then(|function| function.instructions.get(pc))
                        .ok_or(VmError::UnsupportedInstruction(
                            "missing boundary instruction",
                        ))?;
                    let result = self.dispatch_instruction(instruction);
                    self.report_operation(result)?
                }
            };
            match progress {
                InstructionProgress::Continue => {}
                InstructionProgress::Call | InstructionProgress::Await => action = None,
                InstructionProgress::Return(value) => {
                    let result = self
                        .stack
                        .finish_return(self.runtime, value)
                        .map_err(VmError::RuntimeError);
                    if let Some(value) = self.report_operation(result)? {
                        return Ok(DriveOutcome::Complete(value));
                    }
                    action = None;
                }
            }
        }
    }

    fn report_operation<T>(&self, result: Result<T, VmError>) -> Result<T, VmError> {
        result.map_err(|error| {
            if let Some(reason) = error.invariant_reason() {
                return VmError::RuntimeError(self.runtime.quarantine_execution_invariant(reason));
            }
            match self.runtime.observe_execution(ExecutionEvent::Trap) {
                Ok(()) => error,
                Err(observer_error) => VmError::RuntimeError(observer_error),
            }
        })
    }
}
