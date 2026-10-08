//! Exclusive task activations reuse the owned execution backend.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    gc::roots::RootedValue,
    native::binding::NativeResult,
    session::owned::OwnedExecution,
    task::{CancellationCause, TaskId, control::TaskScopeOwner, store::TaskState},
    value::Value,
};
use std::{mem, sync::atomic::Ordering, task::Waker};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDriveResult {
    Runnable,
    Waiting,
    Complete,
    Stale,
}

pub enum TaskAcquisition<'a> {
    Active(TaskActivation<'a>),
    Inactive(TaskDriveResult),
}

/// The backend must return the execution after each slice. Dropping an unfinished
/// activation requests cancellation and leaves cleanup to the control drain.
pub struct TaskActivation<'a> {
    runtime: &'a Runtime,
    task: TaskId,
    execution: Option<OwnedExecution>,
}

impl TaskActivation<'_> {
    pub fn execution(&self) -> &OwnedExecution {
        self.execution.as_ref().expect("live task activation")
    }

    pub fn park(mut self, runnable: bool) -> NativeResult<TaskDriveResult> {
        self.runtime.require_idle_driver()?;
        let signal = {
            let mut tasks = self.runtime.tasks.borrow_mut();
            let record = tasks
                .tasks
                .get_mut(self.task.0)
                .expect("active task record");
            record.state = TaskState::Owned(self.execution.take().expect("live task activation"));
            record.signal.clone()
        };
        if runnable {
            signal.mark_ready();
        }
        Ok(if runnable {
            TaskDriveResult::Runnable
        } else {
            TaskDriveResult::Waiting
        })
    }

    /// The execution backend has already retired the completed owned execution.
    pub fn complete(mut self, result: NativeResult<RootedValue>) -> NativeResult<TaskDriveResult> {
        self.runtime.require_idle_driver()?;
        self.runtime.publish_task_result(self.task, result)?;
        self.execution.take();
        Ok(TaskDriveResult::Complete)
    }
}

impl Drop for TaskActivation<'_> {
    fn drop(&mut self) {
        if let Some(execution) = self.execution.take() {
            let signal = {
                let mut tasks = self.runtime.tasks.borrow_mut();
                let record = tasks
                    .tasks
                    .get_mut(self.task.0)
                    .expect("active task record");
                record.state = TaskState::Owned(execution);
                record.signal.clone()
            };
            signal.cancel(CancellationCause::Explicit);
        }
    }
}

impl Runtime {
    pub fn task_id(&self, value: &Value) -> NativeResult<TaskId> {
        self.gc().task_snapshot(value).map(|(id, _)| id)
    }

    /// Readiness remains available even when a dispatcher drops a notification.
    pub fn ready_tasks(&self, scope: &TaskScopeOwner) -> NativeResult<Vec<TaskId>> {
        self.validate_scope_owner(scope)?;
        Ok(self
            .tasks
            .borrow()
            .tasks
            .iter()
            .filter(|(_, record)| {
                record.scope == scope.id()
                    && !matches!(record.state, TaskState::Admitting | TaskState::Driving)
                    && record.signal.ready.load(Ordering::Acquire)
            })
            .map(|(id, _)| TaskId(id))
            .collect())
    }

    pub fn acquire_task(
        &self,
        scope: &TaskScopeOwner,
        task: TaskId,
    ) -> NativeResult<TaskAcquisition<'_>> {
        self.require_idle_driver()?;
        self.gc().ensure_no_native_borrow()?;
        self.validate_scope_owner(scope)?;
        let (state, signal, options) = {
            let mut tasks = self.tasks.borrow_mut();
            let Some(record) = tasks.tasks.get_mut(task.0) else {
                return Ok(TaskAcquisition::Inactive(TaskDriveResult::Stale));
            };
            if record.scope != scope.id() {
                return Err(RuntimeError::module_validation(
                    "Task belongs to another scope",
                ));
            }
            match record.state {
                TaskState::Admitting | TaskState::Driving => {
                    return Err(RuntimeError::module_validation("Task is already active"));
                }
                TaskState::Terminal(_) => {
                    return Ok(TaskAcquisition::Inactive(TaskDriveResult::Complete));
                }
                _ => {}
            }
            if !record.signal.ready.swap(false, Ordering::AcqRel) {
                return Ok(TaskAcquisition::Inactive(TaskDriveResult::Waiting));
            }
            (
                mem::replace(&mut record.state, TaskState::Driving),
                record.signal.clone(),
                record.options.clone(),
            )
        };
        let execution = match state {
            TaskState::Queued(factory) => {
                let result = if signal.cancellation.check().is_err() {
                    Err(RuntimeError::new(
                        RuntimeErrorKind::Cancelled,
                        "task cancelled before start",
                    ))
                } else {
                    factory
                        .value
                        .value(self.gc())
                        .ok_or_else(|| RuntimeError::module_validation("task factory root"))
                        .and_then(|value| self.start_owned_factory(&value, options))
                };
                match result {
                    Ok(execution) => {
                        execution.set_waker(&Waker::from(signal.clone()));
                        // Registration observes the new execution's initial
                        // readiness. This activation consumes it; no script or
                        // provider operation has run yet.
                        signal.ready.store(false, Ordering::Release);
                        execution
                    }
                    Err(error) => {
                        self.publish_task_result(task, Err(error))?;
                        return Ok(TaskAcquisition::Inactive(TaskDriveResult::Complete));
                    }
                }
            }
            TaskState::Owned(execution) => execution,
            _ => unreachable!("checked task state"),
        };
        Ok(TaskAcquisition::Active(TaskActivation {
            runtime: self,
            task,
            execution: Some(execution),
        }))
    }
}
