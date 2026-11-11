//! Host-driven execution uses the same interpreter and runtime-owned frame store.
use crate::{
    Runtime,
    error::{RuntimeError, reason::ErrorReason},
    executor::{DriveOutcome, Executor},
    gc::roots::RootedValue,
    module::{ExecutableModule, entry::ProgramEntry},
    native::binding::NativeResult,
    session::{ExecutionOptions, owned::OwnedExecution},
    value::Value,
};

use std::num::NonZeroUsize;

#[derive(Debug)]
pub enum DriveResult {
    Runnable,
    Waiting,
    Complete(Result<RootedValue, RuntimeError>),
}

impl Runtime {
    pub fn start<'a>(
        &mut self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: &[Value],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.validate_executable_module(module)?;
        let entry = entry.into();
        let (module, function) = entry.resolve(module).ok_or_else(|| {
            RuntimeError::new(ErrorReason::MissingFunction(entry.label().to_owned()))
        })?;
        let (module, function) = self.select_function(&module, function)?;
        self.start_owned_execution(&module, function, arguments, options)
    }

    /// Queue a cold Future as a root; drive exactly one layer without flattening.
    pub fn start_future(
        &mut self,
        future: &Value,
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.start_owned_future(future, options)
    }

    /// Returns only at a safe interpreter boundary. Native calls are cooperative.
    pub fn drive(
        &mut self,
        owner: &OwnedExecution,
        slice: NonZeroUsize,
    ) -> Result<DriveResult, RuntimeError> {
        self.drain_retired_executions()?;
        let result = self.drive_with_completion(owner, slice, |_, _| Ok(()))?;
        self.drain_retired_executions()?;
        Ok(result)
    }

    /// Convert under the original active session, then retire before returning a
    /// terminal outcome. The scheduler publishes only after this method returns.
    pub(crate) fn drive_with_completion(
        &mut self,
        owner: &OwnedExecution,
        slice: NonZeroUsize,
        complete: impl FnOnce(&mut Runtime, &RootedValue) -> NativeResult<()>,
    ) -> Result<DriveResult, RuntimeError> {
        let executor = Executor::resume(self, owner)?;
        let mut output = None;
        let outcome = executor.run_slice(slice, |runtime, value| {
            let root = runtime
                .root_value(value)
                .ok_or_else(|| RuntimeError::module_validation("owned execution result root"))?;
            complete(runtime, &root)?;
            output = Some(root);
            Ok(())
        });
        let result = match outcome {
            Ok(DriveOutcome::Runnable) => return Ok(DriveResult::Runnable),
            Ok(DriveOutcome::Waiting) => return Ok(DriveResult::Waiting),
            Ok(DriveOutcome::Complete(_)) => Ok(output.expect("rooted terminal output")),
            Err(error) => Err(error),
        };
        self.finish_owned_execution(owner)?;
        Ok(DriveResult::Complete(result))
    }
}
