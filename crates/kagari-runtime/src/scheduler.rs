//! Runtime-owned cooperative scheduling. Backends execute one selected activation
//! at a time; host notifications carry no heap access and never drive script.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, reason::CancellationReason},
    execution::owned::DriveResult,
    native::binding::NativeResult,
    publication::ActivationMark,
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
    value::carriers::RootedValue,
};
mod lifecycle;
pub mod result;
pub mod tasks;
pub mod wake;
#[cfg(test)]
mod wake_tests;

use std::{
    collections::VecDeque,
    fmt,
    marker::PhantomData,
    num::NonZeroUsize,
    rc::Rc,
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
    sink: Box<dyn CompletionSink>,
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
    queue: VecDeque<WorkKey>,
    invocations: Vec<Invocation>,
    pub(crate) closing: bool,
    pub(crate) closed: bool,
    driving: bool,
    pub(crate) failures: VecDeque<tasks::TaskFailureEvent>,
    pub(crate) suppressed: u64,
    limits: AsyncLimits,
}

impl Scheduler {
    pub(crate) fn new(limits: AsyncLimits) -> Self {
        Self {
            hub: Arc::default(),
            queue: VecDeque::default(),
            invocations: Vec::default(),
            closing: false,
            closed: false,
            driving: false,
            failures: VecDeque::default(),
            suppressed: 0,
            limits,
        }
    }

    pub(crate) fn reserve_work(&mut self) -> NativeResult<()> {
        self.queue
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("scheduler ready records"))
    }

    pub(crate) fn enqueue(&mut self, key: WorkKey) {
        self.queue.push_back(key);
        self.hub.mark_ready();
    }

    pub(crate) fn forget(&mut self, key: WorkKey) {
        self.queue.retain(|entry| *entry != key);
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
    _thread: PhantomData<Rc<()>>,
    runtime: &'runtime mut Runtime,
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

    /// Lend the selected execution and the same Runtime to a backend. Borrowed
    /// storage cannot escape this access; dropping the activation still retires it.
    pub fn with_execution<T>(
        &mut self,
        access: impl FnOnce(&mut Runtime, &OwnedExecution) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let invocation = self.invocation.as_ref().expect("active invocation");
        access(self.runtime, &invocation.execution)
    }

    pub(crate) fn drive(&mut self, slice: NonZeroUsize) -> NativeResult<DriveResult> {
        let invocation = self.invocation.as_mut().expect("active invocation");
        self.runtime
            .drive_with_completion(&invocation.execution, slice, |runtime, value| {
                let hub = runtime.scheduler.hub.clone();
                let result = hub.contain(|| invocation.sink.prepare(runtime, value));
                runtime.check_scheduler_fault()?;
                result.expect("contained converter")?;
                runtime.resources().poll_execution()
            })
    }

    pub fn park(mut self) {
        self.runtime
            .scheduler
            .invocations
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
            return Err(self.runtime.quarantine_execution_invariant(
                EngineFault::PublicationBeforeInvocationRetirement,
            ));
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
    runtime: &'runtime mut Runtime,
    activation: Option<ActivationMark>,
}

impl SchedulerTurn<'_> {
    /// Stable rotation across all admitted work. Waiting records retain position;
    /// stale identities are discarded without exposing them to host scheduling.
    pub fn next_work(&mut self) -> NativeResult<Option<ScheduledWork<'_>>> {
        let runtime = &mut *self.runtime;
        runtime.require_idle_driver()?;
        runtime.check_scheduler_fault()?;
        let count = runtime.scheduler.queue.len();
        for _ in 0..count {
            let key = runtime.scheduler.queue.pop_front().expect("queued work");
            match key {
                WorkKey::Invocation(id) => {
                    let selected = {
                        let invocations = &mut runtime.scheduler.invocations;
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
                    runtime.scheduler.queue.push_back(key);
                    if let Some(invocation) = selected {
                        if invocation.execution.is_cancelled() {
                            runtime.retire_invocation(invocation, cancelled());
                            return Ok(None);
                        }
                        return Ok(Some(ScheduledWork::Invocation(InvocationActivation {
                            _thread: Default::default(),
                            runtime,
                            invocation: Some(invocation),
                        })));
                    }
                }
                WorkKey::Group(id) => {
                    runtime.scheduler.queue.push_back(key);
                    match runtime.acquire_group(id.0)? {
                        GroupWork::Branch {
                            parent,
                            index,
                            execution,
                        } => {
                            return Ok(Some(ScheduledWork::Branch(BranchActivation::new(
                                runtime, parent, index, execution,
                            ))));
                        }
                        GroupWork::Control => return Ok(Some(ScheduledWork::Control)),
                        GroupWork::Waiting => {}
                        GroupWork::Stale => runtime.scheduler.forget(key),
                    }
                }
                WorkKey::Task(id) => {
                    let ready = {
                        let tasks = &runtime.tasks;
                        let Some(record) = tasks.tasks.get(id.0) else {
                            continue;
                        };
                        record.signal.ready.load(Ordering::Acquire)
                    };
                    runtime.scheduler.queue.push_back(key);
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
        let cleanup = TurnRetirement {
            runtime: self.runtime,
            activation: self.activation.take(),
        };
        cleanup.runtime.acknowledge_task_scopes();
        let hub = cleanup.runtime.scheduler.hub.clone();
        hub.contain(|| cleanup.runtime.release_dropped_task_scopes());
        hub.finish_turn(cleanup.runtime.scheduler_has_ready_work());
    }
}

/// Keep admission through destructor callbacks without borrowing Runtime fields
/// across them; retire the turn even if local cleanup unwinds.
struct TurnRetirement<'runtime> {
    runtime: &'runtime mut Runtime,
    activation: Option<ActivationMark>,
}

impl Drop for TurnRetirement<'_> {
    fn drop(&mut self) {
        if let Some(mark) = self.activation.take() {
            mark.retire(self.runtime.resources());
        }
        self.runtime.scheduler.driving = false;
    }
}

impl Runtime {
    pub(crate) fn ensure_scheduler_open(&self) -> NativeResult<()> {
        self.check_scheduler_fault()?;
        if self.scheduler.closing {
            return Err(RuntimeError::from(CancellationReason::RuntimeIsClosed));
        }
        Ok(())
    }

    /// Check notification/cleanup faults at a backend turn boundary.
    pub fn check_scheduler_health(&self) -> NativeResult<()> {
        self.check_scheduler_fault()
    }

    pub(crate) fn check_scheduler_fault(&self) -> NativeResult<()> {
        self.scheduler.hub.check().map_err(|_| {
            self.quarantine_execution_invariant(EngineFault::SchedulerCallbackFailed)
        })?;
        if self.is_quarantined() {
            self.resources().ensure_execution_allowed()?;
        }
        Ok(())
    }

    /// Transfer a parked execution to the common scheduler. Conversion executes
    /// once under its original context; publication follows guarded retirement.
    /// Registration failure also retires the transferred execution locally.
    pub fn schedule_execution<R, F>(
        &mut self,
        execution: OwnedExecution,
        convert: F,
    ) -> NativeResult<ScheduledResult<R>>
    where
        R: Send + 'static,
        F: FnOnce(&mut Runtime, &RootedValue) -> NativeResult<R> + Send + 'static,
    {
        let admission = (|| {
            self.require_idle_driver()?;
            self.ensure_scheduler_open()?;
            self.validate_owned_execution(&execution)?;
            self.scheduler.reserve_work()?;
            let invocations = &mut self.scheduler.invocations;
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
        let invocation = Invocation { execution, sink };
        if let Err(error) = self.check_scheduler_fault() {
            self.retire_invocation(invocation, error.clone());
            let _ = self.close();
            return Err(error);
        }
        self.scheduler.invocations.push(invocation);
        self.scheduler.enqueue(WorkKey::Invocation(id));
        Ok(ticket)
    }

    /// Synchronous cancellation for an exclusive borrowed-call drop path. This
    /// releases local state without executing script or waiting for external IO.
    pub fn cancel_scheduled_execution<R>(
        &mut self,
        ticket: &ScheduledResult<R>,
    ) -> NativeResult<()> {
        self.require_idle_driver()?;
        if !ticket.belongs_to(&self.scheduler.hub) {
            return Err(RuntimeError::module_validation("foreign scheduled result"));
        }
        let invocation = {
            let invocations = &mut self.scheduler.invocations;
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
        &mut self,
        cx: &Context<'_>,
        driver: Option<&DriverRegistration>,
    ) -> NativeResult<SchedulerTurn<'_>> {
        self.require_idle_driver()?;
        self.ensure_scheduler_open()?;
        if self.scheduler.driving {
            return Err(RuntimeError::module_validation(
                "scheduler turn already active",
            ));
        }
        let activation = self.resources().begin_synchronous_activation()?.detach();
        self.scheduler.driving = true;
        let activation = Some(activation);
        let turn = SchedulerTurn {
            runtime: self,
            activation,
        };
        match driver {
            Some(driver) => turn.runtime.scheduler.hub.update(driver, cx.waker())?,
            None => turn.runtime.scheduler.hub.register_external(cx.waker())?,
        }
        turn.runtime.scheduler.hub.begin_turn();
        Ok(turn)
    }

    pub fn scheduler_is_closed(&self) -> bool {
        self.scheduler.closed
    }

    fn scheduler_has_ready_work(&self) -> bool {
        self.scheduler
            .invocations
            .iter()
            .any(|entry| entry.execution.is_ready())
            || self.groups_have_ready_work()
            || self.tasks.tasks.iter().any(|(_, task)| {
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
    RuntimeError::from(CancellationReason::ScheduledExecutionCancelled)
}
