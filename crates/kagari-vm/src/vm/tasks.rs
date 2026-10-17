//! Scope ownership is checked before using the ordinary bounded VM driver.
use crate::{
    error::VmError,
    vm::{Vm, owned::DriveResult},
};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    task::{
        TaskId,
        control::TaskScopeOwner,
        drive::{TaskAcquisition, TaskDriveResult},
    },
};
use std::num::NonZeroUsize;

impl Vm {
    pub fn drive_task(
        &self,
        scope: &TaskScopeOwner,
        task: TaskId,
        slice: NonZeroUsize,
    ) -> Result<TaskDriveResult, VmError> {
        let active = match self.runtime.acquire_task(scope, task)? {
            TaskAcquisition::Inactive(result) => return Ok(result),
            TaskAcquisition::Active(active) => active,
        };
        Ok(match self.drive(active.execution(), slice)? {
            DriveResult::Runnable => active.park(true)?,
            DriveResult::Waiting => active.park(false)?,
            DriveResult::Complete(outcome) => active.complete(outcome.map_err(task_failure))?,
        })
    }
}

fn task_failure(error: VmError) -> RuntimeError {
    let trace = error.trace().cloned();
    let result = match error {
        VmError::Traced { error, .. } => task_failure(*error),
        VmError::RuntimeError(error) => error,
        VmError::BuiltinError(error) => error.into_runtime_error(),
        VmError::ReflectionError(error) => error.into_runtime_error(),
        VmError::HostError(error) => RuntimeError::host_call_failure(error.message()),
        VmError::Trap(message) => RuntimeError::new(RuntimeErrorKind::ScriptTrap, message),
        error => RuntimeError::new(
            RuntimeErrorKind::EngineFault,
            format!("task backend failure: {error:?}"),
        ),
    };
    match trace {
        Some(trace) => result.with_trace(trace),
        None => result,
    }
}
