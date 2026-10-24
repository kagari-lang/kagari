//! Local shutdown and cancellation never resume a script continuation.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    native::binding::NativeResult,
    scheduler::{Invocation, WorkKey, cancelled},
    session::owned::OwnedExecution,
    task::CancellationCause,
};
use std::mem;

impl Runtime {
    pub(super) fn publish_invocation(&self, invocation: Invocation, outcome: NativeResult<()>) {
        if outcome
            .as_ref()
            .is_err_and(|error| error.kind() == RuntimeErrorKind::EngineFault)
        {
            self.quarantine_execution_invariant("scheduled invocation failed");
        }
        self.scheduler
            .forget(WorkKey::Invocation(invocation.execution.execution_id()));
        self.scheduler
            .hub
            .contain(|| invocation.sink.into_inner().publish(outcome));
        self.scheduler.hub.contain(|| drop(invocation.execution));
    }

    pub(super) fn retire_invocation(&self, invocation: Invocation, error: RuntimeError) {
        if self.require_idle_driver().is_err() {
            // A backend abandoned its selection before releasing the activation.
            // Keep cleanup ownership until that active scope has unwound.
            self.scheduler.hub.contain(|| invocation.execution.cancel());
            self.scheduler.invocations.borrow_mut().push(invocation);
            self.scheduler.hub.mark_ready();
            return;
        }
        let error = self.retire_scheduled_execution(&invocation.execution, error);
        self.publish_invocation(invocation, Err(error));
    }

    /// Preserve the selected execution's failure and origin unless retirement
    /// itself fails. Entry failures can precede stack activation; cleanup failures
    /// can instead arrive after the backend has already removed the session.
    pub(crate) fn retire_scheduled_execution(
        &self,
        execution: &OwnedExecution,
        error: RuntimeError,
    ) -> RuntimeError {
        let id = execution.id;
        let error = self.group_fatal_error(id).unwrap_or(error);
        let exists = self.resources().sessions.get(id).is_some();
        let trace = error
            .trace()
            .cloned()
            .or_else(|| exists.then(|| ErrorTrace::capture_session(self.resources(), id)));
        let error = if self.is_quarantined() && error.kind() != RuntimeErrorKind::EngineFault {
            self.resources()
                .ensure_execution_allowed()
                .expect_err("quarantined execution")
        } else {
            error
        };
        if error.kind() == RuntimeErrorKind::EngineFault {
            self.quarantine_execution_invariant("scheduled execution failed");
        }
        let cleanup = if exists {
            self.finish_owned_execution(execution)
        } else if self.is_quarantined() {
            // The backend may have discovered this fault while retiring.
            Ok(())
        } else {
            Err(self.quarantine_execution_invariant("missing scheduled execution"))
        };
        let error = cleanup.err().unwrap_or(error);
        match trace {
            Some(trace) => error.with_trace(trace),
            None => error,
        }
    }

    /// Stop admission and synchronously retire local work, including low-level
    /// owners. No factory, continuation or remote cancellation acknowledgement is
    /// run here. Completed receiver values remain available. Cleanup faults are
    /// quarantined and do not prevent retiring other owners. Repeated close is safe.
    pub fn close(&self) -> NativeResult<()> {
        self.require_idle_driver()?;
        self.gc().ensure_no_native_borrow()?;
        if self.scheduler.driving.get() {
            return Err(RuntimeError::module_validation(
                "close during scheduler turn",
            ));
        }
        self.scheduler.closing.set(true);
        if self.scheduler.closed.get() {
            return self.check_scheduler_fault();
        }
        // Mark teardown before callbacks: a reentrant close cannot repeat it.
        self.scheduler.closed.set(true);
        self.modules().retire_installations();
        let mut failure = self.check_scheduler_fault().err();
        let controls = self
            .tasks
            .borrow()
            .scopes
            .iter()
            .map(|(_, scope)| scope.control.clone())
            .collect::<Vec<_>>();
        for control in controls {
            self.scheduler
                .hub
                .contain(|| control.close(CancellationCause::RuntimeShutdown));
        }
        if let Err(error) = self.check_scheduler_fault() {
            failure.get_or_insert(error);
        }
        if let Some(Err(error)) = self.scheduler.hub.contain(|| self.drain_cancelled_tasks()) {
            failure.get_or_insert(error);
        }
        if let Err(error) = self.check_scheduler_fault() {
            failure.get_or_insert(error);
        }
        let invocations = mem::take(&mut *self.scheduler.invocations.borrow_mut());
        for invocation in invocations {
            let error = failure.clone().unwrap_or_else(cancelled);
            self.retire_invocation(invocation, error);
            if let Err(error) = self.check_scheduler_fault() {
                failure.get_or_insert(error);
            }
        }
        if let Err(error) = self.drain_owned_executions(true) {
            failure.get_or_insert(error);
        }
        self.scheduler.queue.borrow_mut().clear();
        self.scheduler.hub.close();
        if let Err(error) = self.check_scheduler_fault() {
            failure.get_or_insert(error);
        }
        if let Some(error) = failure {
            if error.kind() != RuntimeErrorKind::EngineFault {
                return Err(self.quarantine_execution_invariant("runtime shutdown cleanup failed"));
            }
            return Err(error);
        }
        self.resources().close_execution();
        Ok(())
    }
}
