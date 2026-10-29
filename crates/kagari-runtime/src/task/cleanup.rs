//! Cancellation control never invokes a factory or resumes script instructions.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    native::binding::NativeResult,
    session::ExecutionPhase,
    task::{CancellationCause, TaskId, store::TaskState},
    value::Value,
};
use std::{mem, sync::atomic::Ordering};

impl Runtime {
    /// Cancelling a terminal Task is a no-op, including after report retirement.
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn cancel_task_transport(&self, value: &Value) -> NativeResult<()> {
        self.cancel_task(value)
    }

    pub(crate) fn cancel_task(&self, value: &Value) -> NativeResult<()> {
        self.resources().poll_execution()?;
        if self.execution_options().phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation("task cancellation"));
        }
        let (id, outcome) = self.gc().task_snapshot(value)?;
        if outcome.is_some() {
            return Ok(());
        }
        let signal = self
            .tasks
            .borrow()
            .tasks
            .get(id.0)
            .ok_or_else(|| RuntimeError::module_validation("retired unfinished Task"))?
            .signal
            .clone();
        signal.cancel(CancellationCause::Explicit);
        Ok(())
    }

    /// Drain cancelled tasks even if their dispatcher is unavailable. This is a
    /// cleanup capability, not authority to run ready work from another scope.
    pub fn drain_cancelled_tasks(&self) -> NativeResult<usize> {
        self.require_idle_driver()?;
        self.gc().ensure_no_storage_borrow()?;
        let fault = self
            .resources()
            .ensure_execution_allowed()
            .err()
            .filter(|error| error.kind() == RuntimeErrorKind::EngineFault);
        if fault.is_some() {
            for (_, scope) in self.tasks.borrow().scopes.iter() {
                scope.control.closing.store(true, Ordering::Release);
            }
        }
        let ids = self
            .tasks
            .borrow()
            .tasks
            .iter()
            .filter(|(_, task)| {
                matches!(task.state, TaskState::Queued(_) | TaskState::Owned(_))
                    && (fault.is_some() || task.signal.cancellation.check().is_err())
            })
            .map(|(id, _)| TaskId(id))
            .collect::<Vec<_>>();
        let count = ids.len();
        let mut failure = None;
        for id in ids {
            let state = {
                let mut tasks = self.tasks.borrow_mut();
                let record = tasks.tasks.get_mut(id.0).expect("cancelled task record");
                mem::replace(&mut record.state, TaskState::Driving)
            };
            let (cleanup, trace, selected) = match state {
                TaskState::Owned(execution) => {
                    let selected = self.group_fatal_error(execution.id);
                    let trace = selected
                        .as_ref()
                        .and_then(|error| error.trace().cloned())
                        .unwrap_or_else(|| {
                            ErrorTrace::capture_session(self.resources(), execution.id)
                        });
                    // Quarantine may already have retired all owned sessions.
                    let cleanup = if self.resources().sessions.get(execution.id).is_some() {
                        self.finish_owned_execution(&execution)
                    } else if fault.is_some() {
                        Ok(())
                    } else {
                        Err(RuntimeError::module_validation("missing Task execution"))
                    };
                    (cleanup, Some(trace), selected)
                }
                TaskState::Queued(_) => (Ok(()), None, None),
                _ => unreachable!("checked cancellation state"),
            };
            let error = cleanup
                .err()
                .or(selected)
                .or_else(|| fault.clone())
                .unwrap_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::Cancelled, "task cancelled")
                });
            let error = match trace {
                Some(trace) => error.with_trace(trace),
                None => error,
            };
            if let Err(error) = self.publish_task_result(id, Err(error)) {
                failure.get_or_insert(error);
            }
        }
        self.acknowledge_task_scopes();
        self.release_dropped_task_scopes();
        failure.map_or(Ok(count), Err)
    }

    pub(crate) fn release_dropped_task_scopes(&self) {
        let mut tasks = self.tasks.borrow_mut();
        let ids = tasks
            .scopes
            .iter()
            .filter(|(_, scope)| {
                scope.control.owner_dropped.load(Ordering::Acquire)
                    && scope.control.closed.load(Ordering::Acquire)
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let mut discarded_tasks = vec![];
        let mut discarded_scopes = vec![];
        for scope in ids {
            let members = tasks
                .tasks
                .iter()
                .filter(|(_, task)| task.scope.0 == scope)
                .map(|(id, _)| id)
                .collect::<Vec<_>>();
            for task in members {
                discarded_tasks.push(tasks.tasks.remove(task));
            }
            discarded_scopes.push(tasks.scopes.remove(scope));
        }
        drop(tasks);
        // User dispatcher destructors must run outside the registry borrow.
        drop(discarded_tasks);
        drop(discarded_scopes);
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.modules().retire_installations();
        let _ = self.close();
    }
}
