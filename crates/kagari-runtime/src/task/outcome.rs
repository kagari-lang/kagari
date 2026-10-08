//! Terminal publication precedes notification and keeps cached values in the heap.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::asynchronous::AsyncBoundary,
    gc::roots::RootedValue,
    native::binding::NativeResult,
    task::{
        CancellationCause, TaskFailure, TaskFailureOrigin, TaskId, TaskReport,
        control::TaskScopeOwner, store::TaskState,
    },
    value::Value,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
    sync::atomic::Ordering,
};

impl Runtime {
    pub(crate) fn publish_task_result(
        &self,
        task: TaskId,
        result: NativeResult<RootedValue>,
    ) -> NativeResult<()> {
        let (handle, signal, owner, output, spawn_origin, scope) = {
            let tasks = self.tasks.borrow();
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
        let checked = match &result {
            Ok(root) => (|| {
                let value = root
                    .value(self.gc())
                    .ok_or_else(|| RuntimeError::module_validation("Task result root"))?;
                if !output.matches(self, &value, &owner) {
                    return Err(RuntimeError::module_validation("Task result type"));
                }
                self.gc().validate_async_values(slice::from_ref(&value))?;
                Ok(value)
            })(),
            Err(error) => Err(error.clone()),
        };
        let result = checked.map_err(|error| {
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
                        origin: spawn_origin,
                    }),
                source_task: origin.task,
            }
        });
        // Reserve the report's own root before publishing completion. Report
        // consumption then remains a transfer after an unrelated quarantine.
        let report = match &result {
            Ok(value) => Ok(self
                .root_value(value.clone())
                .ok_or_else(|| RuntimeError::module_validation("Task report result root"))?),
            Err(error) => Err(error.clone()),
        };
        let Some(Value::GcHandle(id)) = handle.value(self.gc()) else {
            return Err(RuntimeError::module_validation("Task handle root"));
        };
        self.gc().complete_task(id, task, result)?;
        signal.finished.store(true, Ordering::Release);
        signal.ready.store(true, Ordering::Release);
        self.tasks
            .borrow_mut()
            .tasks
            .get_mut(task.0)
            .expect("completing task")
            .state = TaskState::Terminal(report);
        self.acknowledge_task_scopes();
        let dependencies = self.tasks.borrow().dependencies.clone();
        let mut failure = None;
        for wake in dependencies.complete(task) {
            if catch_unwind(AssertUnwindSafe(|| wake.wake())).is_err() {
                failure = Some(self.quarantine_execution_invariant("Task waiter wake failed"));
            }
        }
        if let Some(scope) = signal.scope.upgrade() {
            let _ = scope.notify(Some(task));
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Taking the bounded report releases its admission slot. Script Task handles
    /// retain their cached value independently and remain awaitable afterwards.
    pub fn take_task_report(
        &self,
        scope: &TaskScopeOwner,
        task: TaskId,
    ) -> NativeResult<Option<TaskReport>> {
        self.require_idle_driver()?;
        self.validate_scope_owner(scope)?;
        let outcome = {
            let tasks = self.tasks.borrow();
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
        let record = self
            .tasks
            .borrow_mut()
            .tasks
            .remove(task.0)
            .expect("reported Task");
        Ok(Some(TaskReport {
            origin: record.origin,
            task,
            scope: scope.id(),
            outcome,
        }))
    }

    pub(crate) fn acknowledge_task_scopes(&self) {
        let tasks = self.tasks.borrow();
        for (id, scope) in tasks.scopes.iter() {
            if scope.control.closing.load(Ordering::Acquire)
                && !tasks.tasks.iter().any(|(_, task)| {
                    task.scope.0 == id && !matches!(task.state, TaskState::Terminal(_))
                })
            {
                scope.control.closed.store(true, Ordering::Release);
            }
        }
    }
}
