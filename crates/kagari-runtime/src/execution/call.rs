//! Driving calls borrow Runtime; submitted calls retain only result/cancellation state.
use crate::{
    Runtime,
    native::binding::NativeResult,
    scheduler::{TurnBudget, result::ScheduledResult, wake::DriverRegistration},
};

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Result-only submitted invocation. Polling never executes script. Keep driving
/// Runtime separately; drop requests cancellation and runtime teardown resolves it.
#[must_use = "drive Runtime and await this result, or drop it to request cancellation"]
#[derive(Debug)]
pub struct PendingCall<R> {
    pub(super) result: ScheduledResult<R>,
}

impl<R> PendingCall<R> {
    pub fn is_finished(&self) -> bool {
        self.result.is_finished()
    }

    pub fn cancel(&self) {
        self.result.cancel();
    }
}

impl<R> Future for PendingCall<R> {
    type Output = NativeResult<R>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().result).poll(cx)
    }
}

/// A direct-await invocation that drives the common bounded scheduler. Its
/// exclusive borrow prevents other direct access until completion or drop.
/// Dropping unfinished work synchronously releases local frames and descendants,
/// even if no external driver was ever installed. Cleanup faults quarantine Runtime.
#[must_use = "await the call to drive it; dropping it cancels and cleans up locally"]
pub struct Call<'runtime, R> {
    runtime: &'runtime mut Runtime,
    pending: PendingCall<R>,
    driver: Option<DriverRegistration>,
    finished: bool,
}

impl<'runtime, R> Call<'runtime, R> {
    pub(super) fn new(runtime: &'runtime mut Runtime, pending: PendingCall<R>) -> Self {
        Self {
            runtime,
            pending,
            driver: None,
            finished: false,
        }
    }

    fn finish(&mut self, result: NativeResult<R>) -> Poll<NativeResult<R>> {
        self.finished = true;
        self.driver.take();
        if let Err(fault) = self.runtime.check_scheduler_health() {
            let mut failure = fault;
            match result {
                Ok(output) => self.pending.result.discard_output(output),
                Err(error) => {
                    if let Some(trace) = error.trace() {
                        failure = failure.with_trace(trace.clone());
                    }
                    if let Some(origin) = error.task_origin() {
                        failure = failure.with_task_origin(origin);
                    }
                }
            }
            let _ = self.runtime.close();
            return Poll::Ready(Err(failure));
        }
        Poll::Ready(result)
    }
}

impl<R> Future for Call<'_, R> {
    type Output = NativeResult<R>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        assert!(!this.finished, "completed Call polled again");
        if this.driver.is_none() {
            match this.runtime.borrow_scheduler_driver(cx) {
                Ok(driver) => this.driver = Some(driver),
                Err(error) => {
                    let _ = this
                        .runtime
                        .cancel_scheduled_execution(&this.pending.result);
                    return this.finish(Err(error));
                }
            }
        }
        if let Poll::Ready(result) = Pin::new(&mut this.pending).poll(cx) {
            return this.finish(result);
        }
        let turn = this
            .runtime
            .poll_scheduler(cx, TurnBudget::default(), this.driver.as_ref());
        if let Poll::Ready(result) = Pin::new(&mut this.pending).poll(cx) {
            return this.finish(result);
        }
        if let Poll::Ready(Err(error)) = turn {
            let _ = this
                .runtime
                .cancel_scheduled_execution(&this.pending.result);
            return this.finish(Err(error));
        }
        Poll::Pending
    }
}

impl<R> Drop for Call<'_, R> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self
                .runtime
                .cancel_scheduled_execution(&self.pending.result);
        }
        self.driver.take();
        if self.runtime.check_scheduler_health().is_err() {
            let _ = self.runtime.close();
        }
    }
}

#[cfg(test)]
mod tests;
