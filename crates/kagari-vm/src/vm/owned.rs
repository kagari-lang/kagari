//! Host-driven execution uses the same interpreter and runtime-owned frame store.
use crate::{
    error::VmError,
    executor::{DriveOutcome, Executor},
    vm::{Vm, find_function_ref},
};
use kagari_runtime::{
    error::RuntimeError,
    gc::roots::RootedValue,
    module::LoadedModule,
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
    pub fn start(
        &self,
        module: &LoadedModule,
        entry: &str,
        arguments: &[Value],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, VmError> {
        let entry = find_function_ref(&module.bytecode, entry)?;
        Ok(self
            .runtime
            .start_owned_execution(module, entry, arguments, options)?)
    }

    /// Returns only at a safe interpreter boundary. Native calls are cooperative.
    pub fn drive(
        &self,
        owner: &OwnedExecution,
        slice: NonZeroUsize,
    ) -> Result<DriveResult, VmError> {
        self.runtime.drain_retired_executions()?;
        let executor = Executor::resume(&self.runtime, owner)?;
        let outcome = executor.run_slice(slice);
        let result = match outcome {
            Ok(DriveOutcome::Runnable) => return Ok(DriveResult::Runnable),
            Ok(DriveOutcome::Waiting) => return Ok(DriveResult::Waiting),
            Ok(DriveOutcome::Complete(value)) => self.runtime.root_value(value).ok_or_else(|| {
                VmError::RuntimeError(RuntimeError::module_validation(
                    "owned execution result root",
                ))
            }),
            Err(error) => Err(error),
        };
        self.runtime.finish_owned_execution(owner)?;
        self.runtime.drain_retired_executions()?;
        Ok(DriveResult::Complete(result))
    }
}
