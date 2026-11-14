//! Terminal publication precedes notification and keeps cached values in the heap.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind, fault::EngineFault},
    error_trace::asynchronous::AsyncBoundary,
    native::binding::NativeResult,
    task::{
        CancellationCause, TaskFailure, TaskFailureOrigin, TaskId, TaskReport,
        control::TaskScopeOwner, storage::TaskCachePublication, store::TaskState,
    },
    value::carriers::RootedValue,
    value_check::data::async_result,
};

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::Ordering,
};

impl Runtime {
    pub(crate) fn publish_task_result(
        &mut self,
        task: TaskId,
        result: NativeResult<RootedValue>,
    ) -> NativeResult<()> {
        let (handle, signal, owner, output, spawn_origin, scope) = {
            let tasks = &self.tasks;
            let record = tasks
                .tasks
                .get(task.0)
                .ok_or_else(|| RuntimeError::module_validation("retired Task"))?;
            if !matches!(record.state, TaskState::Driving) {
                return Err(RuntimeError::module_validation("Task completion state"));
            }
            (
                record.value.clone(),
                record.signal.clone(),
                record.owner.clone(),
                record.output.clone(),
                record.origin.clone(),
                record.scope,
            )
        };
        let checked = match result {
            Ok(root) => async_result(self, &root, &output, &owner).map(|()| root),
            Err(error) => Err(error),
        };
        let failure = |error: RuntimeError| {
            let cancelled = error.kind() == RuntimeErrorKind::Cancelled;
            let origin = error.task_origin().unwrap_or(TaskFailureOrigin {
                task,
                cancellation: cancelled.then(|| signal.cause()).flatten(),
            });
            TaskFailure {
                cancellation: if cancelled && origin.task != task {
                    Some(CancellationCause::Dependency)
                } else {
                    origin.cancellation
                },
                error: error
                    .with_task_origin(origin)
                    .with_async_boundary(AsyncBoundary::Spawn {
                        task,
                        scope,
                        origin: spawn_origin.clone(),
                    }),
                source_task: origin.task,
            }
        };
        let outcome = checked.map_err(failure);
        // A successful completion already owns the report lease. Clone shares it,
        // rather than registering an auxiliary root through a bare value.
        let mut report = outcome.clone();
        let mut publication_error = None;
        match self.cache_task_result(&handle, task, outcome) {
            Ok(TaskCachePublication::Stored | TaskCachePublication::HeapUnavailable) => {}
            Err(error) => {
                // A cache publication fault cannot become a successful Host
                // report. Terminate the control plane even if GC cannot reopen.
                publication_error = Some(error.clone());
                let failed = failure(error);
                report = Err(failed.clone());
                if let Err(error) = self.cache_task_result(&handle, task, Err(failed)) {
                    let invariant = self.quarantine_execution_invariant(
                        EngineFault::HostInvariant("Task failure cache publication failed"),
                    );
                    publication_error = Some(invariant);
                    report = Err(failure(error));
                }
            }
        }
        signal.finished.store(true, Ordering::Release);
        signal.ready.store(true, Ordering::Release);
        self.tasks
            .tasks
            .get_mut(task.0)
            .expect("completing task")
            .state = TaskState::Terminal(report);
        self.acknowledge_task_scopes();
        let dependencies = self.dependencies.clone();
        let mut wake_failure = None;
        for wake in dependencies.complete(task) {
            if catch_unwind(AssertUnwindSafe(|| wake.wake())).is_err() {
                wake_failure =
                    Some(self.quarantine_execution_invariant(EngineFault::TaskWaiterWakeFailed));
            }
        }
        if let Some(scope) = signal.scope.upgrade() {
            let _ = scope.notify(Some(task));
        }
        self.retire_managed_task(task);
        wake_failure.or(publication_error).map_or(Ok(()), Err)
    }

    /// Taking the bounded report releases its admission slot. Script Task handles
    /// retain their cached value independently and remain awaitable afterwards.
    pub fn take_task_report(
        &mut self,
        scope: &TaskScopeOwner,
        task: TaskId,
    ) -> NativeResult<Option<TaskReport>> {
        self.require_idle_driver()?;
        self.validate_scope_owner(scope)?;
        let outcome = {
            let tasks = &self.tasks;
            let Some(record) = tasks.tasks.get(task.0) else {
                return Ok(None);
            };
            if record.scope != scope.id() {
                return Err(RuntimeError::module_validation(
                    "Task belongs to another scope",
                ));
            }
            let TaskState::Terminal(outcome) = &record.state else {
                return Ok(None);
            };
            outcome.clone()
        };
        let record = self.tasks.tasks.remove(task.0).expect("reported Task");
        Ok(Some(TaskReport {
            origin: record.origin,
            task,
            scope: scope.id(),
            outcome,
        }))
    }

    pub(crate) fn acknowledge_task_scopes(&self) {
        let tasks = &self.tasks;
        for (id, scope) in tasks.scopes.iter() {
            if scope.control.status.closing.load(Ordering::Acquire)
                && !tasks.tasks.iter().any(|(_, task)| {
                    task.scope.0 == id && !matches!(task.state, TaskState::Terminal(_))
                })
            {
                scope.control.status.closed.store(true, Ordering::Release);
            }
        }
    }
}
