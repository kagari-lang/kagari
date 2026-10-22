//! Scope ownership is checked before using the ordinary bounded VM driver.
use crate::{
    error::VmError,
    vm::{Vm, owned::DriveResult, scheduler::execution_failure},
};
use kagari_runtime::task::{
    TaskId,
    control::TaskScopeOwner,
    drive::{TaskAcquisition, TaskDriveResult},
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
            DriveResult::Complete(outcome) => {
                active.complete(outcome.map_err(execution_failure))?
            }
        })
    }
}
