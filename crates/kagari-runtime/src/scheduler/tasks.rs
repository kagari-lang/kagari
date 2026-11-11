//! Managed scope Tasks share the invocation ready ring and retire automatically.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind, fault::EngineFault},
    error_trace::asynchronous::SpawnOrigin,
    module::ExecutableModule,
    native::binding::NativeResult,
    scheduler::WorkKey,
    session::ExecutionOptions,
    task::{
        ScopeId, TaskFailure, TaskId, control::TaskScopeOwner, drive::TaskAcquisition,
        store::TaskState,
    },
};

use std::{mem, sync::Arc};

/// Detached diagnostics for a managed scope failure. It retains no script result
/// root. Failures also observed by script waiters are intentionally reported.
#[derive(Debug)]
pub struct TaskFailureEvent {
    pub task: TaskId,
    pub scope: ScopeId,
    pub origin: SpawnOrigin,
    pub failure: TaskFailure,
}

/// Bounded failure reporting never holds Task admission capacity hostage.
#[derive(Debug)]
pub enum TaskEvent {
    Failed(Box<TaskFailureEvent>),
    Suppressed { count: u64 },
}

impl Runtime {
    /// Create a scope driven by the common runtime scheduler, without a host
    /// per-Task dispatcher. Legacy `create_task_scope` keeps manual report/drive
    /// ownership until those callers migrate. Scope-owner drop still cancels jobs.
    pub fn create_scheduled_task_scope(
        &mut self,
        owner: &ExecutableModule,
        options: ExecutionOptions,
    ) -> NativeResult<TaskScopeOwner> {
        self.create_task_scope_inner(
            owner,
            options,
            Arc::new(|_| Ok(())),
            Some(self.scheduler.hub.clone()),
        )
    }

    pub(crate) fn acquire_scheduled_task(
        &mut self,
        id: TaskId,
    ) -> NativeResult<TaskAcquisition<'_>> {
        let scope = self
            .tasks
            .tasks
            .get(id.0)
            .ok_or_else(|| RuntimeError::module_validation("retired scheduled Task"))?
            .scope;
        self.acquire_task_inner(scope, id)
    }

    pub(crate) fn retire_managed_task(&mut self, id: TaskId) {
        let record = {
            let tasks = &mut self.tasks;
            let Some(record) = tasks.tasks.get(id.0) else {
                return;
            };
            let managed = record
                .signal
                .scope
                .upgrade()
                .is_some_and(|scope| scope.scheduler.is_some());
            if !managed || !matches!(record.state, TaskState::Terminal(_)) {
                return;
            }
            tasks.tasks.remove(id.0).expect("terminal managed Task")
        };
        self.scheduler.forget(WorkKey::Task(id));
        if let TaskState::Terminal(Err(failure)) = record.state {
            // Includes failures during queued admission, before an activation exists.
            if failure.error.kind() == RuntimeErrorKind::EngineFault {
                self.quarantine_execution_invariant(EngineFault::ScheduledTaskFailed);
            }
            let events = &mut self.scheduler.failures;
            if events.len() < self.scheduler.limits.max_task_failure_events.get()
                && events.try_reserve(1).is_ok()
            {
                events.push_back(TaskFailureEvent {
                    task: id,
                    scope: record.scope,
                    origin: record.origin,
                    failure,
                });
            } else {
                self.scheduler.suppressed = self.scheduler.suppressed.saturating_add(1);
            }
        }
    }

    /// Drain detached failure diagnostics, then an explicit overflow count.
    /// Ignoring this buffer never prevents Task cleanup or reusing admission slots.
    pub fn take_task_event(&mut self) -> Option<TaskEvent> {
        if let Some(event) = self.scheduler.failures.pop_front() {
            return Some(TaskEvent::Failed(Box::new(event)));
        }
        let count = mem::take(&mut self.scheduler.suppressed);
        (count != 0).then_some(TaskEvent::Suppressed { count })
    }
}
