//! Task waits observe a sealed terminal cache and do not drive the target.
use crate::{
    Runtime,
    error::RuntimeError,
    native::binding::NativeResult,
    session::store::SessionId,
    task::{TaskId, dependencies::WaitLease},
    value::Value,
};
use std::task::{Poll, Waker};

#[derive(Debug)]
pub(crate) struct TaskWait {
    task: TaskId,
    lease: Option<WaitLease>,
}

impl TaskWait {
    pub fn cancel(&mut self) {
        self.lease.take();
    }

    pub fn poll(&mut self, runtime: &Runtime, value: &Value) -> NativeResult<Poll<Value>> {
        let (task, outcome) = runtime.gc().task_snapshot(value)?;
        if task != self.task {
            return Err(RuntimeError::module_validation("Task wait identity"));
        }
        match outcome {
            None => Ok(Poll::Pending),
            Some(outcome) => {
                self.lease.take();
                outcome.map(Poll::Ready).map_err(|failure| failure.error)
            }
        }
    }
}

impl Runtime {
    pub(crate) fn begin_task_wait(
        &self,
        value: &Value,
        session: SessionId,
        wake: Waker,
    ) -> NativeResult<TaskWait> {
        let (task, outcome) = self.gc().task_snapshot(value)?;
        let lease = if outcome.is_none() {
            let dependencies = {
                let tasks = self.tasks.borrow();
                if tasks.tasks.get(task.0).is_none() {
                    return Err(RuntimeError::module_validation("missing unfinished Task"));
                }
                tasks.dependencies.clone()
            };
            Some(dependencies.register(session, task, wake)?)
        } else {
            None
        };
        Ok(TaskWait { task, lease })
    }
}
