//! One bounded turn for invocations and scope Tasks over the ordinary executor.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::owned::DriveResult,
    scheduler::{ScheduledWork, TurnBudget, wake::DriverRegistration},
};

use std::{
    num::NonZeroUsize,
    task::{Context, Poll},
};

impl Runtime {
    /// Open/idle is Pending, never shutdown. Runnable work requests another turn;
    /// external waits do not spin. Temporary drivers preserve the external route.
    pub fn poll_scheduler(
        &self,
        cx: &mut Context<'_>,
        budget: TurnBudget,
        driver: Option<&DriverRegistration>,
    ) -> Poll<Result<(), RuntimeError>> {
        if let Err(error) = self.check_scheduler_health() {
            let _ = self.close();
            return Poll::Ready(Err(error));
        }
        if self.scheduler_is_closed() {
            return Poll::Ready(Ok(()));
        }
        match self.run_scheduler_turn(cx, budget, driver) {
            Ok(()) => Poll::Pending,
            Err(error) => {
                if self.is_quarantined() {
                    let _ = self.close();
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
    ) -> Result<(), RuntimeError> {
        let turn = self.begin_scheduler_turn(cx, driver)?;
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
                        Ok(DriveResult::Complete(result)) => active.complete(result)?,
                        Err(error) => active.fail(error)?,
                    }
                }
                ScheduledWork::Invocation(active) => {
                    let result =
                        self.drive_with_completion(active.execution(), grant, |runtime, value| {
                            active.prepare(runtime, value)
                        });
                    match result {
                        Ok(DriveResult::Runnable | DriveResult::Waiting) => active.park(),
                        Ok(DriveResult::Complete(result)) => active.complete(result.map(|_| ()))?,
                        Err(error) => active.fail(error)?,
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
                            active.complete(result)?;
                        }
                        Err(error) => {
                            active.fail(error)?;
                        }
                    }
                }
            }
        }
        drop(turn);
        // Waker destruction/notification can fail at turn exit too.
        self.check_scheduler_health()?;
        Ok(())
    }
}
