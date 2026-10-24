//! Runtime-owned cooperative scheduling. Backends execute one selected activation
//! at a time; host notifications carry no heap access and never drive script.
mod lifecycle;
pub mod result;
pub mod tasks;
pub mod wake;
#[cfg(test)]
mod wake_tests;

use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    gc::roots::RootedValue,
    native::binding::NativeResult,
    publication::SynchronousActivation,
    resource::AsyncLimits,
    scheduler::{
        result::{CompletionSink, ScheduledResult, channel},
        wake::{DriverRegistration, WakeHub},
    },
    session::{
        ExecutionId,
        group::{BranchActivation, GroupWork},
        owned::OwnedExecution,
    },
    task::{
        TaskId,
        drive::{TaskAcquisition, TaskActivation},
        store::TaskState,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fmt,
    num::NonZeroUsize,
    ptr,
    sync::{Arc, atomic::Ordering},
    task::{Context, Waker},
};

/// One shared turn allowance, debited by granted quanta even on early waits.
/// Native calls and non-suspendable operations can exceed a quantum; this is not
/// execution charging or hard preemption. Each activation also costs one step.
#[derive(Debug, Clone, Copy)]
pub struct TurnBudget {
    pub instructions: NonZeroUsize,
    pub quantum: NonZeroUsize,
    pub activations: NonZeroUsize,
}

impl Default for TurnBudget {
    fn default() -> Self {
        Self {
            instructions: NonZeroUsize::new(4096).expect("positive budget"),
            quantum: NonZeroUsize::new(256).expect("positive quantum"),
            activations: NonZeroUsize::new(64).expect("positive activations"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkKey {
    Invocation(ExecutionId),
    Task(TaskId),
    Group(ExecutionId),
}

struct Invocation {
    execution: OwnedExecution,
    sink: RefCell<Box<dyn CompletionSink>>,
}

impl fmt::Debug for Invocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Invocation")
            .field("execution", &self.execution)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(crate) struct Scheduler {
    pub(crate) hub: Arc<WakeHub>,
    queue: RefCell<VecDeque<WorkKey>>,
    invocations: RefCell<Vec<Invocation>>,
    pub(crate) closing: Cell<bool>,
    pub(crate) closed: Cell<bool>,
    driving: Cell<bool>,
    pub(crate) failures: RefCell<VecDeque<tasks::TaskFailureEvent>>,
    pub(crate) suppressed: Cell<u64>,
    limits: AsyncLimits,
}

impl Scheduler {
    pub(crate) fn new(limits: AsyncLimits) -> Self {
        Self {
            hub: Arc::default(),
            queue: RefCell::default(),
            invocations: RefCell::default(),
            closing: Cell::new(false),
            closed: Cell::new(false),
            driving: Cell::new(false),
            failures: RefCell::default(),
            suppressed: Cell::new(0),
            limits,
        }
    }

    pub(crate) fn reserve_work(&self) -> NativeResult<()> {
        self.queue
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("scheduler ready records"))
    }

    pub(crate) fn enqueue(&self, key: WorkKey) {
        self.queue.borrow_mut().push_back(key);
        self.hub.mark_ready();
    }

    pub(crate) fn forget(&self, key: WorkKey) {
        self.queue.borrow_mut().retain(|entry| *entry != key);
    }
}

/// Backend activation, not a public per-Task scheduling protocol.
pub enum ScheduledWork<'runtime> {
    Invocation(InvocationActivation<'runtime>),
    Task(TaskActivation<'runtime>),
    Branch(BranchActivation<'runtime>),
    /// Admission or terminal cleanup consumed one bounded activation.
    Control,
}

/// A selected invocation is detached from scheduler tables before callbacks run.
pub struct InvocationActivation<'runtime> {
    runtime: &'runtime Runtime,
    invocation: Option<Invocation>,
}

impl InvocationActivation<'_> {
    pub fn execution(&self) -> &OwnedExecution {
        &self
            .invocation
            .as_ref()
            .expect("active invocation")
            .execution
    }

    /// Runs inside the original active session, before its stack is retired.
    pub fn prepare(&self, runtime: &Runtime, value: &RootedValue) -> NativeResult<()> {
        if !ptr::eq(runtime, self.runtime) {
            return Err(RuntimeError::module_validation(
                "foreign completion runtime",
            ));
        }
        let invocation = self.invocation.as_ref().expect("active invocation");
        if runtime
            .resources()
            .active_session()
            .is_none_or(|state| state.id != invocation.execution.id)
        {
            return Err(RuntimeError::module_validation(
                "completion requires its active session",
            ));
        }
        let result = runtime
            .scheduler
            .hub
            .contain(|| invocation.sink.borrow_mut().prepare(runtime, value));
        runtime.check_scheduler_fault()?;
        result.expect("contained converter")?;
        runtime.resources().poll_execution()
    }

    pub fn park(mut self) {
        self.runtime
            .scheduler
            .invocations
            .borrow_mut()
            .push(self.invocation.take().expect("active invocation"));
    }

    /// Finish a backend entry/retirement failure without substituting cancellation.
    /// Ordinary failures resolve this invocation only; fatal failures quarantine.
    pub fn fail(mut self, error: RuntimeError) -> NativeResult<()> {
        self.runtime.require_idle_driver()?;
        let invocation = self.invocation.take().expect("active invocation");
        self.runtime.retire_invocation(invocation, error);
        self.runtime.check_scheduler_fault()
    }

    /// Backend retirement must precede publication. A failed retirement overrides
    /// a converted success; no result is published while local frames remain.
    pub fn complete(mut self, outcome: NativeResult<()>) -> NativeResult<()> {
        self.runtime.require_idle_driver()?;
        let invocation = self.invocation.take().expect("active invocation");
        if self
            .runtime
            .resources()
            .sessions
            .get(invocation.execution.id)
            .is_some()
        {
            self.invocation = Some(invocation);
            return Err(self
                .runtime
                .quarantine_execution_invariant("publication before invocation retirement"));
        }
        self.runtime.publish_invocation(invocation, outcome);
        self.runtime.check_scheduler_fault()
    }
}

impl Drop for InvocationActivation<'_> {
    fn drop(&mut self) {
        if let Some(invocation) = self.invocation.take() {
            self.runtime.retire_invocation(invocation, cancelled());
        }
    }
}

/// Exclusive scheduling turn guard. It holds no active script frame or table
/// borrow; synchronous host reentry remains a session operation, not a new turn.
pub struct SchedulerTurn<'runtime> {
    runtime: &'runtime Runtime,
    _activation: SynchronousActivation<'runtime>,
}

impl SchedulerTurn<'_> {
    /// Stable rotation across all admitted work. Waiting records retain position;
    /// stale identities are discarded without exposing them to host scheduling.
    pub fn next_work(&self) -> NativeResult<Option<ScheduledWork<'_>>> {
        let runtime = self.runtime;
        runtime.require_idle_driver()?;
        runtime.check_scheduler_fault()?;
        let count = runtime.scheduler.queue.borrow().len();
        for _ in 0..count {
            let key = runtime
                .scheduler
                .queue
                .borrow_mut()
                .pop_front()
                .expect("queued work");
            match key {
                WorkKey::Invocation(id) => {
                    let selected = {
                        let mut invocations = runtime.scheduler.invocations.borrow_mut();
                        let Some(index) = invocations
                            .iter()
                            .position(|entry| entry.execution.execution_id() == id)
                        else {
                            continue;
                        };
                        if invocations[index].execution.is_ready() {
                            Some(invocations.remove(index))
                        } else {
                            None
                        }
                    };
                    runtime.scheduler.queue.borrow_mut().push_back(key);
                    if let Some(invocation) = selected {
                        if invocation.execution.is_cancelled() {
                            runtime.retire_invocation(invocation, cancelled());
                            return Ok(None);
                        }
                        return Ok(Some(ScheduledWork::Invocation(InvocationActivation {
                            runtime,
                            invocation: Some(invocation),
                        })));
                    }
                }
                WorkKey::Group(id) => {
                    runtime.scheduler.queue.borrow_mut().push_back(key);
                    match runtime.acquire_group(id.0)? {
                        GroupWork::Branch(active) => {
                            return Ok(Some(ScheduledWork::Branch(active)));
                        }
                        GroupWork::Control => return Ok(Some(ScheduledWork::Control)),
                        GroupWork::Waiting => {}
                        GroupWork::Stale => runtime.scheduler.forget(key),
                    }
                }
                WorkKey::Task(id) => {
                    let ready = {
                        let tasks = runtime.tasks.borrow();
                        let Some(record) = tasks.tasks.get(id.0) else {
                            continue;
                        };
                        record.signal.ready.load(Ordering::Acquire)
                    };
                    runtime.scheduler.queue.borrow_mut().push_back(key);
                    if ready {
                        return match runtime.acquire_scheduled_task(id)? {
                            TaskAcquisition::Active(active) => {
                                Ok(Some(ScheduledWork::Task(active)))
                            }
                            TaskAcquisition::Inactive(_) => Ok(None),
                        };
                    }
                }
            }
        }
        Ok(None)
    }
}

impl Drop for SchedulerTurn<'_> {
    fn drop(&mut self) {
        self.runtime.acknowledge_task_scopes();
        self.runtime
            .scheduler
            .hub
            .contain(|| self.runtime.release_dropped_task_scopes());
        self.runtime
            .scheduler
            .hub
            .finish_turn(self.runtime.scheduler_has_ready_work());
        self.runtime.scheduler.driving.set(false);
    }
}

impl Runtime {
    pub(crate) fn ensure_scheduler_open(&self) -> NativeResult<()> {
        self.check_scheduler_fault()?;
        if self.scheduler.closing.get() {
            return Err(RuntimeError::new(
                RuntimeErrorKind::Cancelled,
                "runtime is closed",
            ));
        }
        Ok(())
    }

    /// Check notification/cleanup faults at a backend turn boundary.
    pub fn check_scheduler_health(&self) -> NativeResult<()> {
        self.check_scheduler_fault()
    }

    pub(crate) fn check_scheduler_fault(&self) -> NativeResult<()> {
        self.scheduler
            .hub
            .check()
            .map_err(|_| self.quarantine_execution_invariant("scheduler callback failed"))?;
        if self.is_quarantined() {
            self.resources().ensure_execution_allowed()?;
        }
        Ok(())
    }

    /// Transfer a parked execution to the common scheduler. Conversion executes
    /// once under its original context; publication follows guarded retirement.
    /// Registration failure also retires the transferred execution locally.
    pub fn schedule_execution<R, F>(
        &self,
        execution: OwnedExecution,
        convert: F,
    ) -> NativeResult<ScheduledResult<R>>
    where
        R: Send + 'static,
        F: FnOnce(&Runtime, &RootedValue) -> NativeResult<R> + Send + 'static,
    {
        let admission = (|| {
            self.require_idle_driver()?;
            self.ensure_scheduler_open()?;
            self.validate_owned_execution(&execution)?;
            self.scheduler.reserve_work()?;
            let mut invocations = self.scheduler.invocations.borrow_mut();
            if invocations.len() >= self.scheduler.limits.max_scheduled_invocations.get() {
                return Err(RuntimeError::resource_limit("scheduled invocations"));
            }
            invocations
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("scheduled invocations"))
        })();
        if let Err(error) = admission {
            if self.validate_owned_execution(&execution).is_ok() {
                self.finish_owned_execution(&execution)?;
            }
            return Err(error);
        }
        let id = execution.execution_id();
        let (ticket, sink) = channel(
            id,
            self.scheduler.hub.clone(),
            execution.cancellation_token(),
            convert,
        );
        self.scheduler
            .hub
            .contain(|| execution.set_waker(&Waker::from(self.scheduler.hub.clone())));
        let invocation = Invocation {
            execution,
            sink: RefCell::new(sink),
        };
        if let Err(error) = self.check_scheduler_fault() {
            self.retire_invocation(invocation, error.clone());
            let _ = self.close();
            return Err(error);
        }
        self.scheduler.invocations.borrow_mut().push(invocation);
        self.scheduler.enqueue(WorkKey::Invocation(id));
        Ok(ticket)
    }

    /// Synchronous cancellation for an exclusive borrowed-call drop path. This
    /// releases local state without executing script or waiting for external IO.
    pub fn cancel_scheduled_execution<R>(&self, ticket: &ScheduledResult<R>) -> NativeResult<()> {
        self.require_idle_driver()?;
        if !ticket.belongs_to(&self.scheduler.hub) {
            return Err(RuntimeError::module_validation("foreign scheduled result"));
        }
        let invocation = {
            let mut invocations = self.scheduler.invocations.borrow_mut();
            invocations
                .iter()
                .position(|entry| entry.execution.execution_id() == ticket.id)
                .map(|index| invocations.remove(index))
        };
        if let Some(invocation) = invocation {
            self.retire_invocation(invocation, cancelled());
        }
        if let Err(error) = self.check_scheduler_fault() {
            let _ = self.close();
            return Err(error);
        }
        Ok(())
    }

    pub fn borrow_scheduler_driver(&self, cx: &Context<'_>) -> NativeResult<DriverRegistration> {
        self.require_idle_driver()?;
        self.ensure_scheduler_open()?;
        self.scheduler.hub.borrow(cx.waker())
    }

    /// Register the current driver before observing readiness. `None` updates the
    /// persistent external route; a borrowed route does not replace that route.
    pub fn begin_scheduler_turn(
        &self,
        cx: &Context<'_>,
        driver: Option<&DriverRegistration>,
    ) -> NativeResult<SchedulerTurn<'_>> {
        self.require_idle_driver()?;
        self.ensure_scheduler_open()?;
        if self.scheduler.driving.get() {
            return Err(RuntimeError::module_validation(
                "scheduler turn already active",
            ));
        }
        let activation = self.resources().begin_synchronous_activation()?;
        self.scheduler.driving.set(true);
        let turn = SchedulerTurn {
            runtime: self,
            _activation: activation,
        };
        match driver {
            Some(driver) => self.scheduler.hub.update(driver, cx.waker())?,
            None => self.scheduler.hub.register_external(cx.waker())?,
        }
        self.scheduler.hub.begin_turn();
        Ok(turn)
    }

    pub fn scheduler_is_closed(&self) -> bool {
        self.scheduler.closed.get()
    }

    fn scheduler_has_ready_work(&self) -> bool {
        self.scheduler
            .invocations
            .borrow()
            .iter()
            .any(|entry| entry.execution.is_ready())
            || self.groups_have_ready_work()
            || self.tasks.borrow().tasks.iter().any(|(_, task)| {
                task.signal
                    .scope
                    .upgrade()
                    .is_some_and(|scope| scope.scheduler.is_some())
                    && matches!(task.state, TaskState::Queued(_) | TaskState::Owned(_))
                    && task.signal.ready.load(Ordering::Acquire)
            })
    }
}

fn cancelled() -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Cancelled, "scheduled execution cancelled")
}
