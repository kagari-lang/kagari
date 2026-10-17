mod aggregate_ops;
mod dispatch;
mod driver;
pub(crate) mod native;
mod value_ops;

use kagari_bytecode::{instruction::Register, program::ModuleRef};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    RootedInterfaceMethod, Runtime,
    frame::{ExecutionFrame, ExecutionStack},
    module::LoadedModule,
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
