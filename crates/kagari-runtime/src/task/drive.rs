//! Exclusive task activations reuse the owned execution backend.
use crate::{
    Runtime,
    error::{RuntimeError, reason::CancellationReason},
    error_trace::ErrorTrace,
    execution::owned::DriveResult,
    native::binding::NativeResult,
    session::owned::OwnedExecution,
    task::{CancellationCause, ScopeId, TaskId, control::TaskScopeOwner, store::TaskState},
    value::carriers::RootedValue,
};

use std::{
    marker::PhantomData, mem, num::NonZeroUsize, rc::Rc, sync::atomic::Ordering, task::Waker,
};

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
    _thread: PhantomData<Rc<()>>,
    runtime: &'a mut Runtime,
    task: TaskId,
    execution: Option<OwnedExecution>,
}

impl TaskActivation<'_> {
    pub fn execution(&self) -> &OwnedExecution {
        self.execution.as_ref().expect("live task activation")
    }

    pub fn with_execution<T>(
        &mut self,
        access: impl FnOnce(&mut Runtime, &OwnedExecution) -> NativeResult<T>,
    ) -> NativeResult<T> {
        access(
            self.runtime,
            self.execution.as_ref().expect("live task activation"),
        )
    }

    pub(crate) fn drive(&mut self, slice: NonZeroUsize) -> NativeResult<DriveResult> {
        self.runtime.drive_with_completion(
            self.execution.as_ref().expect("live task activation"),
            slice,
            |_, _| Ok(()),
        )
    }

    pub fn park(mut self, runnable: bool) -> NativeResult<TaskDriveResult> {
        self.runtime.require_idle_driver()?;
        let signal = {
            let tasks = &mut self.runtime.tasks;
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

    /// Retire a failed scheduler activation and publish its original error, with
    /// cleanup faults taking precedence. No continuation is resumed for cleanup.
    pub fn fail(self, error: RuntimeError) -> NativeResult<TaskDriveResult> {
        self.runtime.require_idle_driver()?;
        let error = self
            .runtime
            .retire_scheduled_execution(self.execution.as_ref().expect("active execution"), error);
        self.complete(Err(error))
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
                let tasks = &mut self.runtime.tasks;
                let Some(record) = tasks.tasks.get_mut(self.task.0) else {
                    return;
                };
                if matches!(record.state, TaskState::Terminal(_)) {
                    return;
                }
                record.state = TaskState::Owned(execution);
                record.signal.clone()
            };
            signal.cancel(CancellationCause::Explicit);
        }
    }
}

impl Runtime {
    pub fn task_id(&mut self, value: &RootedValue) -> NativeResult<TaskId> {
        self.task_state(value).map(|(id, _)| id)
    }

    /// Readiness remains available even when a dispatcher drops a notification.
    pub fn ready_tasks(&self, scope: &TaskScopeOwner) -> NativeResult<Vec<TaskId>> {
        self.validate_scope_owner(scope)?;
        Ok(self
            .tasks
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
        &mut self,
        scope: &TaskScopeOwner,
        task: TaskId,
    ) -> NativeResult<TaskAcquisition<'_>> {
        self.require_idle_driver()?;
        self.validate_scope_owner(scope)?;
        if scope.control.scheduler.is_some() {
            return Err(RuntimeError::module_validation(
                "managed Tasks use the runtime scheduler",
            ));
        }
        self.acquire_task_inner(scope.id(), task)
    }

    pub(crate) fn acquire_task_inner(
        &mut self,
        scope: ScopeId,
        task: TaskId,
    ) -> NativeResult<TaskAcquisition<'_>> {
        self.require_idle_driver()?;
        let (state, signal, options) = {
            let tasks = &mut self.tasks;
            let Some(record) = tasks.tasks.get_mut(task.0) else {
                return Ok(TaskAcquisition::Inactive(TaskDriveResult::Stale));
            };
            if record.scope != scope {
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
                    Err(RuntimeError::from(
                        CancellationReason::TaskCancelledBeforeStart,
                    ))
                } else {
                    self.start_queued_factory(factory, options)
                };
                match result {
                    Ok(execution) => {
                        let dependencies = self.dependencies.clone();
                        if let Err(error) = dependencies.attach(task, execution.id) {
                            self.finish_owned_execution(&execution)?;
                            self.publish_task_result(task, Err(error))?;
                            return Ok(TaskAcquisition::Inactive(TaskDriveResult::Complete));
                        }
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
            TaskState::Owned(execution) => {
                if signal.cancellation.check().is_err()
                    && signal
                        .scope
                        .upgrade()
                        .is_some_and(|scope| scope.scheduler.is_some())
                {
                    let trace = ErrorTrace::capture_session(self.resources(), execution.id);
                    let error = self
                        .finish_owned_execution(&execution)
                        .err()
                        .unwrap_or_else(|| RuntimeError::from(CancellationReason::TaskCancelled));
                    self.publish_task_result(task, Err(error.with_trace(trace)))?;
                    return Ok(TaskAcquisition::Inactive(TaskDriveResult::Complete));
                }
                execution
            }
            _ => unreachable!("checked task state"),
        };
        Ok(TaskAcquisition::Active(TaskActivation {
            _thread: Default::default(),
            runtime: self,
            task,
            execution: Some(execution),
        }))
    }
}
