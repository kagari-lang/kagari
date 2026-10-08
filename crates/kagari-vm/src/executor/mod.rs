mod aggregate_ops;
mod dispatch;
mod loop_body;
pub(crate) mod native;
mod value_ops;

use kagari_bytecode::{
    instruction::{BytecodeInstruction, Register},
    module::CallableTarget,
    program::ModuleRef,
};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    RootedInterfaceMethod, Runtime,
    frame::{ExecutionFrame, ExecutionStack, transfer::ReturnValue},
    module::LoadedModule,
    session::{ExecutionEvent, owned::OwnedExecution},
    value::Value,
};
use std::{
    cell::{Ref, RefMut},
    num::NonZeroUsize,
    task::Poll,
};

use crate::{error::VmError, executor::loop_body::LoopExit};

pub(crate) struct Executor<'a> {
    runtime: &'a Runtime,
    stack: ExecutionStack<'a>,
}

pub(crate) enum DriveOutcome {
    Runnable,
    Waiting,
    Complete(Value),
}

impl<'a> Executor<'a> {
    pub(crate) fn new(
        runtime: &'a Runtime,
        loaded: &'a LoadedModule,
        entry: FunctionRef,
        args: &[Value],
    ) -> Result<Self, VmError> {
        let module = &loaded.bytecode;
        module
            .functions
            .get(entry.index())
            .ok_or(VmError::InvalidFunctionRef(entry))?;

        let mut executor = Self {
            runtime,
            stack: runtime.enter_execution_stack(loaded)?,
        };
        executor.push_frame(loaded.slot(), entry, args, None)?;
        Ok(executor)
    }

    pub(crate) fn new_interface(
        runtime: &'a Runtime,
        method: RootedInterfaceMethod,
        args: &[Value],
    ) -> Result<Self, VmError> {
        let loaded = method.implementation(runtime)?;
        let stack = runtime.enter_execution_stack(&loaded)?;
        stack.push_interface_method(runtime, method, args, None)?;
        Ok(Self { runtime, stack })
    }

    pub(crate) fn run(&mut self) -> Result<Value, VmError> {
        self.run_inner(None)
            .and_then(|outcome| match outcome {
                DriveOutcome::Complete(value) => Ok(value),
                _ => Err(VmError::UnsupportedInstruction(
                    "synchronous entry cannot suspend",
                )),
            })
            .map_err(|error| error.with_trace(self.runtime.capture_error_trace()))
    }

    pub(crate) fn resume(runtime: &'a Runtime, owner: &OwnedExecution) -> Result<Self, VmError> {
        Ok(Self {
            runtime,
            stack: runtime.resume_owned_execution(owner)?,
        })
    }

    pub(crate) fn run_slice(mut self, slice: NonZeroUsize) -> Result<DriveOutcome, VmError> {
        let value = self
            .run_inner(Some(slice.get()))
            .map_err(|error| error.with_trace(self.runtime.capture_error_trace()))?;
        match &value {
            DriveOutcome::Runnable => self.stack.park(self.runtime)?,
            DriveOutcome::Waiting => self.stack.park_waiting(self.runtime)?,
            DriveOutcome::Complete(_) => {}
        }
        Ok(value)
    }

    fn run_inner(&mut self, mut remaining: Option<usize>) -> Result<DriveOutcome, VmError> {
        loop {
            self.runtime.resources().poll_execution()?;
            let resumed = self
                .stack
                .poll_await(self.runtime)
                .map_err(VmError::RuntimeError);
            match self.report_operation(resumed)? {
                Poll::Pending => return Ok(DriveOutcome::Waiting),
                Poll::Ready(Some(value)) => return Ok(DriveOutcome::Complete(value)),
                Poll::Ready(None) => {}
            }
            if remaining == Some(0) && self.stack.can_park(self.runtime)? {
                return Ok(DriveOutcome::Runnable);
            }
            let native_return = self.current_frame()?.native_return(self.runtime)?;
            if let Some(value) = native_return {
                let result = self
                    .stack
                    .finish_return(self.runtime, ReturnValue::general(value));
                if let Some(value) = self.report_operation(result.map_err(VmError::RuntimeError))? {
                    return Ok(DriveOutcome::Complete(value));
                }
                continue;
            }
            if self.current_frame()?.has_pending_native_entry() {
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
                self.report_operation(result)?;
                continue;
            }
            self.current_frame_mut()?.prepare_instruction();
            self.runtime.gc_safepoint().map_err(VmError::RuntimeError)?;
            self.runtime
                .observe_execution(ExecutionEvent::BeforeInstruction)?;

            let result = self.run_cursor(&mut remaining);
            match self.report_operation(result)? {
                LoopExit::Safepoint | LoopExit::Slice => continue,
                LoopExit::Return(value) => {
                    let result = self.stack.finish_return(self.runtime, value);
                    if let Some(value) =
                        self.report_operation(result.map_err(VmError::RuntimeError))?
                    {
                        return Ok(DriveOutcome::Complete(value));
                    }
                }
                LoopExit::Boundary => {
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
                    if let BytecodeInstruction::Return(register) = instruction {
                        let value = register
                            .map(|register| {
                                self.current_frame()?
                                    .read_register(self.runtime, register)
                                    .map_err(VmError::RuntimeError)
                            })
                            .transpose()?
                            .unwrap_or(Value::Unit);
                        let result = self
                            .stack
                            .finish_return(self.runtime, ReturnValue::general(value));
                        if let Some(value) =
                            self.report_operation(result.map_err(VmError::RuntimeError))?
                        {
                            return Ok(DriveOutcome::Complete(value));
                        }
                        continue;
                    }
                    if let BytecodeInstruction::Await { dst, value, future } = instruction {
                        let value = self.current_frame()?.read_register(self.runtime, *value)?;
                        let result = self
                            .stack
                            .begin_await(self.runtime, value, *dst, future)
                            .map_err(VmError::RuntimeError);
                        self.report_operation(result)?;
                        continue;
                    }
                    let result = self.dispatch_instruction(instruction);
                    self.report_operation(result)?;
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

    pub(crate) fn current_loaded(&self) -> Result<LoadedModule, VmError> {
        Ok(self.current_frame()?.loaded().clone())
    }

    pub(crate) fn current_frame(&self) -> Result<Ref<'_, ExecutionFrame>, VmError> {
        Ok(self.stack.current()?)
    }

    pub(crate) fn current_frame_mut(&self) -> Result<RefMut<'_, ExecutionFrame>, VmError> {
        Ok(self.stack.current_mut()?)
    }

    pub(crate) fn push_frame(
        &mut self,
        module: ModuleRef,
        function: FunctionRef,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<(), VmError> {
        Ok(self
            .stack
            .push(self.runtime, module, function, args, return_dst)?)
    }
}
