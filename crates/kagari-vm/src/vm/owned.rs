//! Host-driven execution uses the same interpreter and runtime-owned frame store.
use crate::{
    error::VmError,
    executor::{DriveOutcome, Executor},
    vm::Vm,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
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
    Complete(Result<RootedValue, VmError>),
}

impl Vm {
    pub fn start<'a>(
        &self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
        arguments: &[Value],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, VmError> {
        self.runtime.validate_executable_module(module)?;
        let entry = entry.into();
        let (module, function) = entry
            .resolve(module)
            .ok_or_else(|| VmError::MissingFunction(entry.label().to_owned()))?;
        Ok(self
            .runtime
            .start_owned_execution(&module, function, arguments, options)?)
    }

    /// Queue a cold Future as a root; drive exactly one layer without flattening.
    pub fn start_future(
        &self,
        future: &Value,
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, VmError> {
        Ok(self.runtime.start_owned_future(future, options)?)
    }

    /// Returns only at a safe interpreter boundary. Native calls are cooperative.
    pub fn drive(
        &self,
        owner: &OwnedExecution,
        slice: NonZeroUsize,
    ) -> Result<DriveResult, VmError> {
        self.runtime.drain_retired_executions()?;
        let result = self.drive_with_completion(owner, slice, |_, _| Ok(()))?;
        self.runtime.drain_retired_executions()?;
        Ok(result)
    }

    /// Convert under the original active session, then retire before returning a
    /// terminal outcome. The scheduler publishes only after this method returns.
    pub(super) fn drive_with_completion(
        &self,
        owner: &OwnedExecution,
        slice: NonZeroUsize,
        complete: impl FnOnce(&Runtime, &RootedValue) -> NativeResult<()>,
    ) -> Result<DriveResult, VmError> {
        let executor = Executor::resume(&self.runtime, owner)?;
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
        self.runtime.finish_owned_execution(owner)?;
        Ok(DriveResult::Complete(result))
    }
}
