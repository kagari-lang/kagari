//! Task waits observe a sealed terminal cache and do not drive the target.
use crate::{
    Runtime,
    error::RuntimeError,
    error_trace::{ErrorFrame, asynchronous::AsyncBoundary},
    execution::roots::{SessionId, ValueLocation},
    native::binding::NativeResult,
    session::dependencies::WaitLease,
    task::TaskId,
};
use std::task::{Poll, Waker};

#[derive(Debug)]
pub(crate) struct TaskWait {
    task: TaskId,
    lease: Option<WaitLease>,
    site: Option<ErrorFrame>,
}

impl TaskWait {
    pub fn cancel(&mut self) {
        self.lease.take();
    }

    pub fn poll(
        &mut self,
        runtime: &mut Runtime,
        value: ValueLocation,
        output: ValueLocation,
    ) -> NativeResult<Poll<()>> {
        match runtime.poll_task_result(value, output, self.task)? {
            Poll::Pending => Ok(Poll::Pending),
            Poll::Ready(outcome) => {
                self.lease.take();
                outcome.map(Poll::Ready).map_err(|failure| {
                    failure.error.with_async_boundary(AsyncBoundary::Await {
                        task: self.task,
                        site: self.site.clone(),
                    })
                })
            }
        }
    }
}

impl Runtime {
    pub(crate) fn begin_task_wait(
        &mut self,
        value: ValueLocation,
        session: SessionId,
        wake: Waker,
    ) -> NativeResult<TaskWait> {
        let (task, ready) = self.task_state_at(value)?;
        let site = self.capture_async_site();
        let lease = if !ready {
            let dependencies = {
                let tasks = &self.tasks;
                if tasks.tasks.get(task.0).is_none() {
                    return Err(RuntimeError::module_validation("missing unfinished Task"));
                }
                self.dependencies.clone()
            };
            Some(
                dependencies
                    .register(session, task, wake)
                    .map_err(|error| {
                        error.with_async_boundary(AsyncBoundary::Await {
                            task,
                            site: site.clone(),
                        })
                    })?,
            )
        } else {
            None
        };
        Ok(TaskWait { task, lease, site })
    }
}
