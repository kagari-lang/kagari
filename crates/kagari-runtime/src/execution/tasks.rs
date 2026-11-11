//! Scope ownership is checked before using the ordinary bounded interpreter driver.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::owned::DriveResult,
    task::{
        TaskId,
        control::TaskScopeOwner,
        drive::{TaskAcquisition, TaskDriveResult},
    },
};

use std::num::NonZeroUsize;

impl Runtime {
    pub fn drive_task(
        &self,
        scope: &TaskScopeOwner,
        task: TaskId,
        slice: NonZeroUsize,
    ) -> Result<TaskDriveResult, RuntimeError> {
        let active = match self.acquire_task(scope, task)? {
            TaskAcquisition::Inactive(result) => return Ok(result),
            TaskAcquisition::Active(active) => active,
        };
        Ok(match self.drive(active.execution(), slice)? {
            DriveResult::Runnable => active.park(true)?,
            DriveResult::Waiting => active.park(false)?,
            DriveResult::Complete(outcome) => active.complete(outcome)?,
        })
    }
}
