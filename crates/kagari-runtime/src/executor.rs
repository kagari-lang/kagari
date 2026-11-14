use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::{ExecutionFrame, ExecutionStack, FrameStack, transfer::ReturnValue},
    module::ExecutableModule,
    native::conversion::arguments::ArgumentValues,
    session::owned::OwnedExecution,
};
mod dispatch;
mod driver;
pub(crate) mod native;

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

pub(crate) enum DriveOutcome<T> {
    Runnable,
    Waiting,
    Complete(T),
}

impl<'a> Executor<'a> {
    pub(crate) fn new(
        runtime: &'a mut Runtime,
        loaded: &'a ExecutableModule,
        entry: FunctionRef,
        args: ArgumentValues<'_>,
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
        method: &RootedInterfaceMethod,
        args: ArgumentValues<'_>,
    ) -> Result<Self, RuntimeError> {
        let mut activation = runtime.enter_interface_execution_stack(method)?;
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

    /// Consume the result while its execution source and authority remain live.
    /// A Host consumer may retain it; an internal consumer publishes or decodes
    /// directly without introducing an escaping root lease.
    pub(crate) fn run<R>(
        &mut self,
        complete: impl FnOnce(&mut Runtime, &FrameStack, ReturnValue) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        self.activation.drive(|runtime, stack| {
            let result = Interpreter { runtime, stack }.run_inner(None);
            let result = result.and_then(|outcome| match outcome {
                DriveOutcome::Complete(value) => complete(
                    runtime,
                    stack.as_ref().expect("completed active stack"),
                    value,
                ),
                _ => Err(RuntimeError::from(EngineFault::HostInvariant(
                    "synchronous entry cannot suspend",
                ))),
            });
            result.map_err(|error| {
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

    pub(crate) fn run_slice<R>(
        mut self,
        slice: NonZeroUsize,
        complete: impl FnOnce(&mut Runtime, &FrameStack, ReturnValue) -> Result<R, RuntimeError>,
    ) -> Result<DriveOutcome<R>, RuntimeError> {
        self.activation.drive(|runtime, stack| {
            let result = (|| match (Interpreter { runtime, stack }).run_inner(Some(slice.get()))? {
                DriveOutcome::Runnable => {
                    debug_assert!(stack.is_none());
                    Ok(DriveOutcome::Runnable)
                }
                DriveOutcome::Waiting => {
                    FrameStack::park_waiting(stack, runtime)?;
                    Ok(DriveOutcome::Waiting)
                }
                DriveOutcome::Complete(value) => complete(
                    runtime,
                    stack.as_ref().expect("completed active stack"),
                    value,
                )
                .map(DriveOutcome::Complete),
            })();
            result.map_err(|error: RuntimeError| {
                error
                    .admit_execution_error(runtime.resources())
                    .with_trace(runtime.capture_error_trace())
            })
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
