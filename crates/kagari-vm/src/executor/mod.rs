mod aggregate_ops;
mod dispatch;
mod driver;
pub(crate) mod native;
mod value_ops;

use kagari_bytecode::module::CallableTarget;
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    RootedInterfaceMethod, Runtime,
    frame::{ExecutionFrame, ExecutionStack},
    module::ExecutableModule,
    session::owned::OwnedExecution,
    value::Value,
};
use std::{
    cell::{Ref, RefMut},
    num::NonZeroUsize,
};

use crate::error::VmError;

pub(crate) struct Executor<'a> {
    runtime: &'a Runtime,
    stack: Option<ExecutionStack<'a>>,
}

pub(crate) enum DriveOutcome {
    Runnable,
    Waiting,
    Complete(Value),
}

impl<'a> Executor<'a> {
    pub(crate) fn new(
        runtime: &'a Runtime,
        loaded: &'a ExecutableModule,
        entry: FunctionRef,
        args: &[Value],
    ) -> Result<Self, VmError> {
        let module = &loaded.bytecode;
        module
            .functions
            .get(entry.index())
            .ok_or(VmError::InvalidFunctionRef(entry))?;

        let executor = Self {
            runtime,
            stack: Some(runtime.enter_execution_stack(loaded)?),
        };
        executor
            .stack()
            .push_callable(
                runtime,
                loaded.clone(),
                CallableTarget::Script(entry),
                args,
                None,
            )
            .map_err(|error| runtime.resources().admit_execution_error(error))?;
        Ok(executor)
    }

    pub(crate) fn new_interface(
        runtime: &'a Runtime,
        method: RootedInterfaceMethod,
        args: &[Value],
    ) -> Result<Self, VmError> {
        let loaded = method.implementation(runtime)?;
        let stack = runtime.enter_execution_stack(&loaded)?;
        stack
            .push_interface_method(runtime, method, args, None)
            .map_err(|error| runtime.resources().admit_execution_error(error))?;
        Ok(Self {
            runtime,
            stack: Some(stack),
        })
    }

    pub(crate) fn run(&mut self) -> Result<Value, VmError> {
        self.run_inner(None)
            .and_then(|outcome| match outcome {
                DriveOutcome::Complete(value) => Ok(value),
                _ => Err(VmError::UnsupportedInstruction(
                    "synchronous entry cannot suspend",
                )),
            })
            .map_err(|error| {
                error
                    .admit_execution_error(self.runtime.resources())
                    .with_trace(self.runtime.capture_error_trace())
            })
    }

    pub(crate) fn resume(runtime: &'a Runtime, owner: &OwnedExecution) -> Result<Self, VmError> {
        Ok(Self {
            runtime,
            stack: Some(runtime.resume_owned_execution(owner)?),
        })
    }

    pub(crate) fn run_slice(
        mut self,
        slice: NonZeroUsize,
        complete: impl FnOnce(&Runtime, Value) -> Result<(), VmError>,
    ) -> Result<DriveOutcome, VmError> {
        let value = self.run_inner(Some(slice.get())).map_err(|error| {
            error
                .admit_execution_error(self.runtime.resources())
                .with_trace(self.runtime.capture_error_trace())
        })?;
        match &value {
            DriveOutcome::Runnable => debug_assert!(self.stack.is_none()),
            DriveOutcome::Waiting => ExecutionStack::park_waiting(&mut self.stack, self.runtime)
                .map_err(|error| {
                    VmError::from(error)
                        .admit_execution_error(self.runtime.resources())
                        .with_trace(self.runtime.capture_error_trace())
                })?,
            DriveOutcome::Complete(value) => complete(self.runtime, *value).map_err(|error| {
                error
                    .admit_execution_error(self.runtime.resources())
                    .with_trace(self.runtime.capture_error_trace())
            })?,
        }
        Ok(value)
    }

    fn stack(&self) -> &ExecutionStack<'a> {
        self.stack.as_ref().expect("active executor")
    }

    pub(crate) fn current_loaded(&self) -> Result<ExecutableModule, VmError> {
        Ok(self.current_frame()?.loaded().clone())
    }

    pub(crate) fn current_frame(&self) -> Result<Ref<'_, ExecutionFrame>, VmError> {
        Ok(self.stack().current()?)
    }

    pub(crate) fn current_frame_mut(&self) -> Result<RefMut<'_, ExecutionFrame>, VmError> {
        Ok(self.stack().current_mut()?)
    }
}
