//! One bounded turn for invocations and scope Tasks over the ordinary executor.
use crate::{
    error::VmError,
    vm::{Vm, owned::DriveResult},
};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    scheduler::{ScheduledWork, TurnBudget, wake::DriverRegistration},
};
use std::{
    num::NonZeroUsize,
    task::{Context, Poll},
};

impl Vm {
    /// Open/idle is Pending, never shutdown. Runnable work requests another turn;
    /// external waits do not spin. Temporary drivers preserve the external route.
    pub fn poll_scheduler(
        &self,
        cx: &mut Context<'_>,
        budget: TurnBudget,
        driver: Option<&DriverRegistration>,
    ) -> Poll<Result<(), VmError>> {
        if let Err(error) = self.runtime.check_scheduler_health() {
            let _ = self.runtime.close();
            return Poll::Ready(Err(VmError::from(error)));
        }
        if self.runtime.scheduler_is_closed() {
            return Poll::Ready(Ok(()));
        }
        match self.run_scheduler_turn(cx, budget, driver) {
            Ok(()) => Poll::Pending,
            Err(error) => {
                if self.runtime.is_quarantined() {
                    let _ = self.runtime.close();
                }
                Poll::Ready(Err(error))
            }
        }
    }

    fn run_scheduler_turn(
        &self,
        cx: &Context<'_>,
        budget: TurnBudget,
        driver: Option<&DriverRegistration>,
    ) -> Result<(), VmError> {
        let turn = self.runtime.begin_scheduler_turn(cx, driver)?;
        let mut remaining = budget.instructions.get();
        for _ in 0..budget.activations.get() {
            let Some(grant) = NonZeroUsize::new(remaining.min(budget.quantum.get())) else {
                break;
            };
            remaining -= grant.get();
            let Some(work) = turn.next_work()? else { break };
            match work {
                ScheduledWork::Control => {}
                ScheduledWork::Branch(active) => {
                    match self.drive_with_completion(active.execution(), grant, |_, _| Ok(())) {
                        Ok(DriveResult::Runnable | DriveResult::Waiting) => active.park()?,
                        Ok(DriveResult::Complete(result)) => {
                            active.complete(result.map_err(execution_failure))?
                        }
                        Err(error) => active.fail(execution_failure(error))?,
                    }
                }
                ScheduledWork::Invocation(active) => {
                    let result =
                        self.drive_with_completion(active.execution(), grant, |runtime, value| {
                            active.prepare(runtime, value)
                        });
                    match result {
                        Ok(DriveResult::Runnable | DriveResult::Waiting) => active.park(),
                        Ok(DriveResult::Complete(result)) => {
                            active.complete(result.map(|_| ()).map_err(execution_failure))?
                        }
                        Err(error) => active.fail(execution_failure(error))?,
                    }
                }
                ScheduledWork::Task(active) => {
                    match self.drive_with_completion(active.execution(), grant, |_, _| Ok(())) {
                        Ok(DriveResult::Runnable) => {
                            active.park(true)?;
                        }
                        Ok(DriveResult::Waiting) => {
                            active.park(false)?;
                        }
                        Ok(DriveResult::Complete(result)) => {
                            active.complete(result.map_err(execution_failure))?;
                        }
                        Err(error) => {
                            active.fail(execution_failure(error))?;
                        }
                    }
                }
            }
        }
        drop(turn);
        // Waker destruction/notification can fail at turn exit too.
        self.runtime.check_scheduler_health()?;
        Ok(())
    }
}

pub(super) fn execution_failure(error: VmError) -> RuntimeError {
    let trace = error.trace().cloned();
    let result = match error {
        VmError::Traced { error, .. } => execution_failure(*error),
        VmError::RuntimeError(error) => error,
        VmError::BuiltinError(error) => error.into_runtime_error(),
        VmError::ReflectionError(error) => error.into_runtime_error(),
        VmError::HostError(error) => RuntimeError::host_call_failure(error.message()),
        VmError::Trap(message) => RuntimeError::new(RuntimeErrorKind::ScriptTrap, message),
        error => RuntimeError::new(
            RuntimeErrorKind::EngineFault,
            format!("owned backend failure: {error:?}"),
        ),
    };
    match trace {
        Some(trace) => result.with_trace(trace),
        None => result,
    }
}
