use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::{ExecutionFrame, ExecutionStack, FrameStack},
    module::ExecutableModule,
    session::owned::OwnedExecution,
    value::Value,
};
mod aggregate_ops;
mod dispatch;
mod driver;
pub(crate) mod native;
mod value_ops;

use kagari_bytecode::module::CallableTarget;
use kagari_contract::ids::FunctionRef;
use std::{
    cell::{Ref, RefMut},
    num::NonZeroUsize,
};

pub(crate) struct Executor<'a> {
    activation: ExecutionStack<'a>,
}

struct Interpreter<'a> {
    runtime: &'a mut Runtime,
    stack: &'a mut Option<FrameStack>,
}

pub(crate) enum DriveOutcome {
    Runnable,
    Waiting,
    Complete(Value),
}

impl<'a> Executor<'a> {
    pub(crate) fn new(
        runtime: &'a mut Runtime,
        loaded: &'a ExecutableModule,
        entry: FunctionRef,
        args: &[Value],
    ) -> Result<Self, RuntimeError> {
        loaded
            .bytecode
            .functions
            .get(entry.index())
            .ok_or(RuntimeError::from(
                EngineFault::VerifiedFunctionReferenceIsMissing,
            ))?;
        let mut activation = runtime.enter_execution_stack(loaded)?;
        activation.with(|runtime, stack| {
            stack
                .push_callable(
                    runtime,
                    loaded.clone(),
                    CallableTarget::Script(entry),
                    args,
                    None,
                )
                .map_err(|error| runtime.resources().admit_execution_error(error))
        })?;
        Ok(Self { activation })
    }

    pub(crate) fn new_interface(
        runtime: &'a mut Runtime,
        method: RootedInterfaceMethod,
        args: &[Value],
    ) -> Result<Self, RuntimeError> {
        let loaded = method.implementation(runtime)?;
        let mut activation = runtime.enter_execution_stack(&loaded)?;
        activation.with(|runtime, stack| {
            stack
                .push_interface_method(runtime, method, args, None)
                .map_err(|error| runtime.resources().admit_execution_error(error))
        })?;
        Ok(Self { activation })
    }

    pub(crate) fn from_activation(activation: ExecutionStack<'a>) -> Self {
        Self { activation }
    }

    pub(crate) fn run(&mut self) -> Result<Value, RuntimeError> {
        self.activation.drive(|runtime, stack| {
            Interpreter { runtime, stack }
                .run_inner(None)
                .and_then(|outcome| match outcome {
                    DriveOutcome::Complete(value) => Ok(value),
                    _ => Err(RuntimeError::from(EngineFault::HostInvariant(
                        "synchronous entry cannot suspend",
                    ))),
                })
                .map_err(|error| {
                    error
                        .admit_execution_error(runtime.resources())
                        .with_trace(runtime.capture_error_trace())
                })
        })
    }

    pub(crate) fn resume(
        runtime: &'a mut Runtime,
        owner: &OwnedExecution,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            activation: runtime.resume_owned_execution(owner)?,
        })
    }

    pub(crate) fn run_slice(
        mut self,
        slice: NonZeroUsize,
        complete: impl FnOnce(&mut Runtime, Value) -> Result<(), RuntimeError>,
    ) -> Result<DriveOutcome, RuntimeError> {
        self.activation.drive(|runtime, stack| {
            let value = Interpreter { runtime, stack }
                .run_inner(Some(slice.get()))
                .map_err(|error| {
                    error
                        .admit_execution_error(runtime.resources())
                        .with_trace(runtime.capture_error_trace())
                })?;
            match &value {
                DriveOutcome::Runnable => debug_assert!(stack.is_none()),
                DriveOutcome::Waiting => {
                    FrameStack::park_waiting(stack, runtime).map_err(|error| {
                        error
                            .admit_execution_error(runtime.resources())
                            .with_trace(runtime.capture_error_trace())
                    })?
                }
                DriveOutcome::Complete(value) => complete(runtime, *value).map_err(|error| {
                    error
                        .admit_execution_error(runtime.resources())
                        .with_trace(runtime.capture_error_trace())
                })?,
            }
            Ok(value)
        })
    }
}

impl Interpreter<'_> {
    fn stack(&self) -> &FrameStack {
        self.stack.as_ref().expect("active executor")
    }

    pub(crate) fn current_loaded(&self) -> Result<ExecutableModule, RuntimeError> {
        Ok(self.current_frame()?.loaded().clone())
    }

    pub(crate) fn current_frame(&self) -> Result<Ref<'_, ExecutionFrame>, RuntimeError> {
        self.stack().current(self.runtime)
    }

    pub(crate) fn current_frame_mut(&self) -> Result<RefMut<'_, ExecutionFrame>, RuntimeError> {
        self.stack().current_mut(self.runtime)
    }
}
