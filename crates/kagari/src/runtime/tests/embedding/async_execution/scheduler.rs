//! Notification-driven integration, not a busy-poll/per-Task host scheduler.
use super::Fixture;
use crate::{context::ExecutionContext, engine::EngineConfig, runtime::Runtime};
use kagari_runtime::{
    Runtime as CoreRuntime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    gc::roots::RootedValue,
    module::LoadedModule,
    native::{
        binding::NativeResult, completion::CompletionStatus, conversion::context::ConversionContext,
    },
    scheduler::{TurnBudget, result::ScheduledResult, tasks::TaskEvent, wake::DriverRegistration},
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use std::{
    cell::Cell,
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
    time::Duration,
};

struct Notify {
    pending: AtomicBool,
    send: SyncSender<()>,
}

impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.send.try_send(()).expect("live coalesced host adapter");
        }
    }
}

pub(super) struct HostLoop {
    notify: Arc<Notify>,
    receive: Receiver<()>,
    wake: Waker,
}

impl HostLoop {
    pub(super) fn new() -> Self {
        let (send, receive) = mpsc::sync_channel(1);
        let notify = Arc::new(Notify {
            pending: AtomicBool::new(false),
            send,
        });
        Self {
            wake: Waker::from(notify.clone()),
            notify,
            receive,
        }
    }

    pub(super) fn context(&self) -> Context<'_> {
        Context::from_waker(&self.wake)
    }

    fn acknowledge(&self) {
        self.notify.pending.store(false, Ordering::Release);
    }

    pub(super) fn turn(
        &self,
        runtime: &mut Runtime,
        budget: TurnBudget,
        driver: Option<&DriverRegistration>,
    ) {
        self.receive
            .recv_timeout(Duration::from_secs(2))
            .expect("runtime lost a wake");
        self.acknowledge();
        assert!(
            runtime
                .poll_scheduler(&mut self.context(), budget, driver)
                .is_pending()
        );
    }

    pub(super) fn until(&self, runtime: &mut Runtime, done: impl Fn() -> bool) {
        for _ in 0..200 {
            if done() {
                return;
            }
            self.turn(runtime, TurnBudget::default(), None);
        }
        panic!("bounded event-loop progress");
    }

    pub(super) fn settle(&self, runtime: &mut Runtime) {
        for _ in 0..200 {
            if self.receive.try_recv().is_err() {
                return;
            }
            self.acknowledge();
            assert!(runtime.poll(&mut self.context()).is_pending());
        }
        panic!("idle runtime self-wakes continuously");
    }
}

const SOURCE: &str = r#"
use test::async_sdk::request;
fn admitted(result:Result<Task<i32>,SpawnError>) -> Task<i32> {
    match result { Ok(task) => task, Err(_) => { val zero=0; val bad=1/zero; admitted(result) } }
}
pub fn launch(scope:TaskScope) -> Task<i32> { admitted(scope.spawn(async || request(10).await)) }
pub fn fast(scope:TaskScope) -> Task<i32> { admitted(scope.spawn(async || 17)) }
pub fn busy(scope:TaskScope) -> Task<i32> { admitted(scope.spawn(async || long().await)) }
pub fn fail(scope:TaskScope) -> Task<i32> { admitted(scope.spawn(async || { val zero=0; 1/zero })) }
pub async fn observe(task:Task<i32>) -> i32 { task.await + 1 }
pub async fn io(value:i32) -> i32 { request(value).await }
pub async fn long() -> i32 { var n=0; while n < 200 { n+=1; } n }
pub fn value() -> i32 { 42 }
"#;

pub(super) fn decode(runtime: &CoreRuntime, value: &RootedValue) -> NativeResult<i32> {
    let owner = runtime
        .execution_root()
        .expect("conversion retains active invocation");
    ConversionContext::new(runtime, &owner)?.decode(value)
}

pub(super) fn scheduled(f: &Fixture, entry: &str, arguments: &[Value]) -> ScheduledResult<i32> {
    let future = f.invoke(entry, arguments);
    f.runtime
        .runtime()
        .schedule_execution(f.start_future(&future), decode)
        .unwrap()
}

pub(super) fn result<R>(ticket: &mut ScheduledResult<R>) -> NativeResult<R> {
    match Pin::new(ticket).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("terminal receiver expected"),
    }
}

#[test]
fn scheduler_scope_io_cached_results_and_retirement() {
    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_tasks = NonZeroUsize::new(1).unwrap();
    let mut f = Fixture::configured_source(SOURCE, config);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    assert!(
        host.receive.try_recv().is_err(),
        "open idle is not runnable"
    );
    let scope = f
        .runtime
        .create_task_scope_with_context(&f.module, &Default::default())
        .unwrap();
    let task = f.invoke("launch", &[f.value(scope.capability())]);
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "handler returned before task body"
    );
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 1);
    host.settle(&mut f.runtime);
    // The scope task started without any scheduled host invocation present.
    let mut observer = scheduled(&f, "observe", &[f.value(&task)]);
    host.settle(&mut f.runtime);
    assert!(!observer.is_finished());
    let completion = f.sent.lock().unwrap().pop().unwrap();
    thread::spawn(move || {
        assert_eq!(completion.complete(Ok(40)), CompletionStatus::Accepted);
        assert_eq!(completion.complete(Ok(99)), CompletionStatus::Duplicate);
    })
    .join()
    .unwrap();
    host.until(&mut f.runtime, || observer.is_finished());
    assert_eq!(result(&mut observer).unwrap(), 41);
    // No host TaskId report was consumed. The cached result survives retirement.
    let another = f.invoke("fast", &[f.value(scope.capability())]);
    let mut second = scheduled(&f, "observe", &[f.value(&another)]);
    let mut cached = scheduled(&f, "observe", &[f.value(&task)]);
    host.until(&mut f.runtime, || {
        second.is_finished() && cached.is_finished()
    });
    assert_eq!(result(&mut second).unwrap(), 18);
    assert_eq!(result(&mut cached).unwrap(), 41);
    assert!(f.runtime.take_task_event().is_none());
    host.settle(&mut f.runtime);
    let _cancelled = f.invoke("launch", &[f.value(scope.capability())]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 2);
    let late = f.sent.lock().unwrap().pop().unwrap();
    drop(scope);
    host.until(&mut f.runtime, || f.cancels.load(Ordering::SeqCst) == 1);
    assert_eq!(late.complete(Ok(99)), CompletionStatus::Stale);
    host.settle(&mut f.runtime);
}

#[test]
fn scheduler_shared_budget_and_driver_handoff() {
    let mut f = Fixture::source(SOURCE);
    let external = HostLoop::new();
    let borrowed = HostLoop::new();
    assert!(f.runtime.poll(&mut external.context()).is_pending());
    let mut left = scheduled(&f, "long", &[]);
    let mut right = scheduled(&f, "io", &[Value::I32(10)]);
    let driver = f
        .runtime
        .runtime()
        .borrow_scheduler_driver(&borrowed.context())
        .unwrap();
    let budget = TurnBudget {
        instructions: NonZeroUsize::new(1).unwrap(),
        quantum: NonZeroUsize::new(1).unwrap(),
        activations: NonZeroUsize::new(1).unwrap(),
    };
    for _ in 0..100 {
        if f.starts.load(Ordering::SeqCst) != 0 {
            break;
        }
        borrowed.turn(&mut f.runtime, budget, Some(&driver));
    }
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "round robin cannot starve IO behind a long root"
    );
    assert!(
        !left.is_finished(),
        "one child cannot receive a full outer allowance repeatedly"
    );
    drop(driver);
    external.until(&mut f.runtime, || left.is_finished());
    assert_eq!(result(&mut left).unwrap(), 200);
    external.settle(&mut f.runtime);
    let done = f.sent.lock().unwrap().pop().unwrap();
    done.complete(Ok(9));
    external.until(&mut f.runtime, || right.is_finished());
    assert_eq!(result(&mut right).unwrap(), 9);
    external.settle(&mut f.runtime);
}

#[test]
fn scheduler_conversion_context_results_and_local_cancellation() {
    fn send<T: Send>() {}
    send::<Runtime>();
    send::<ScheduledResult<Cell<i32>>>();
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    let waiter = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut options = ExecutionContext::default();
    options.inputs.unix_time_millis = 123;
    let execution = f
        .runtime
        .backend()
        .start(
            &f.module,
            "value",
            &[],
            options.interpreter_options().unwrap(),
        )
        .unwrap();
    let id = execution.execution_id();
    let mut ticket = f
        .runtime
        .runtime()
        .schedule_execution(execution, move |runtime, value| {
            assert_eq!(runtime.execution_time_millis()?, 123);
            assert!(runtime.execution_root().is_some());
            Ok(Cell::new(decode(runtime, value)?))
        })
        .unwrap();
    assert_eq!(ticket.execution_id(), id);
    assert!(
        Pin::new(&mut ticket)
            .poll(&mut waiter.context())
            .is_pending()
    );
    host.until(&mut f.runtime, || ticket.is_finished());
    waiter.receive.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        f.runtime.runtime().execution_root().is_none(),
        "published only after activation exit"
    );
    assert_eq!(result(&mut ticket).unwrap().get(), 42);
    let mut before = scheduled(&f, "io", &[Value::I32(10)]);
    f.runtime
        .runtime()
        .cancel_scheduled_execution(&before)
        .unwrap();
    assert_eq!(
        result(&mut before).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    drop(scheduled(&f, "io", &[Value::I32(11)]));
    host.settle(&mut f.runtime);
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "receiver drop before first drive is cold"
    );
    let mut pending = scheduled(&f, "io", &[Value::I32(10)]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 1);
    let late = f.sent.lock().unwrap().pop().unwrap();
    f.runtime
        .runtime()
        .cancel_scheduled_execution(&pending)
        .unwrap();
    // No subsequent scheduler poll is necessary for provider cancellation.
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(late.complete(Ok(1)), CompletionStatus::Stale);
    assert_eq!(
        result(&mut pending).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    let abandoned = scheduled(&f, "io", &[Value::I32(11)]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 2);
    drop(abandoned);
    host.until(&mut f.runtime, || f.cancels.load(Ordering::SeqCst) == 2);
    host.settle(&mut f.runtime);
}

#[test]
fn scheduler_failure_overflow_shutdown_and_ticket_teardown() {
    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_tasks = NonZeroUsize::new(1).unwrap();
    config.default_runtime.async_limits.max_task_failure_events = NonZeroUsize::new(1).unwrap();
    let mut f = Fixture::configured_source(SOURCE, config);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let scope = f
        .runtime
        .create_task_scope_with_context(&f.module, &Default::default())
        .unwrap();
    for _ in 0..3 {
        let task = f.invoke("fail", &[f.value(scope.capability())]);
        let mut observe = scheduled(&f, "observe", &[f.value(&task)]);
        host.until(&mut f.runtime, || observe.is_finished());
        assert_eq!(
            result(&mut observe).unwrap_err().kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
    assert!(matches!(
        f.runtime.take_task_event(),
        Some(TaskEvent::Failed(_))
    ));
    assert!(matches!(
        f.runtime.take_task_event(),
        Some(TaskEvent::Suppressed { count: 2 })
    ));
    let mut pending = scheduled(&f, "io", &[Value::I32(10)]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 1);
    let late = f.sent.lock().unwrap().pop().unwrap();
    f.runtime.close().unwrap();
    f.runtime.close().unwrap();
    assert!(matches!(
        f.runtime.poll(&mut host.context()),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(
        result(&mut pending).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(late.complete(Ok(9)), CompletionStatus::Stale);
    assert!(scope.is_closed());
    assert!(
        f.runtime
            .backend()
            .start(&f.module, "value", &[], Default::default())
            .is_err()
    );
    let mut other = Fixture::source(SOURCE);
    let mut ticket = scheduled(&other, "io", &[Value::I32(10)]);
    assert!(other.runtime.poll(&mut host.context()).is_pending());
    drop(other);
    assert_eq!(
        result(&mut ticket).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
}

#[test]
fn scheduler_receiver_drop_during_conversion_retires_output() {
    struct Converted(Arc<AtomicUsize>);

    impl Drop for Converted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let mut f = Fixture::source(SOURCE);
    let drops = Arc::new(AtomicUsize::new(0));
    let converted_drops = drops.clone();
    let (converting, started) = mpsc::sync_channel(1);
    let (abandoned, dropped) = mpsc::sync_channel(1);
    let execution = f
        .runtime
        .backend()
        .start(&f.module, "value", &[], Default::default())
        .unwrap();
    let ticket = f
        .runtime
        .runtime()
        .schedule_execution(execution, move |runtime, value| {
            assert_eq!(decode(runtime, value)?, 42);
            converting.send(()).unwrap();
            dropped.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(Converted(converted_drops))
        })
        .unwrap();
    let worker = thread::spawn(move || {
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(ticket);
        abandoned.send(()).unwrap();
    });
    assert!(
        f.runtime
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    worker.join().unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(!f.runtime.runtime().is_quarantined());
    assert!(f.runtime.runtime().execution_root().is_none());
    f.runtime.close().unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct RejectEntry(Option<RuntimeError>);

impl ExecutionObserver for RejectEntry {
    fn begin(&mut self, _: &CoreRuntime, _: &LoadedModule) -> NativeResult<()> {
        self.0.take().map_or(Ok(()), Err)
    }

    fn observe(
        &mut self,
        _: &CoreRuntime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> NativeResult<()> {
        Ok(())
    }
}

#[test]
fn scheduler_entry_failure_is_an_outcome_and_preserves_other_work() {
    for (task_entry, resume_task) in [(false, false), (true, false), (true, true)] {
        let mut f = Fixture::source(SOURCE);
        let host = HostLoop::new();
        let mut pending = scheduled(&f, "io", &[Value::I32(10)]);
        assert!(f.runtime.poll(&mut host.context()).is_pending());
        host.settle(&mut f.runtime);
        let late = f.sent.lock().unwrap().pop().unwrap();
        let scope = f
            .runtime
            .create_task_scope_with_context(&f.module, &Default::default())
            .unwrap();
        let mut rejected = if task_entry {
            let task = f.invoke("busy", &[f.value(scope.capability())]);
            if resume_task {
                let one = NonZeroUsize::new(1).unwrap();
                assert!(
                    f.runtime
                        .poll_scheduler(
                            &mut host.context(),
                            TurnBudget {
                                instructions: one,
                                quantum: one,
                                activations: one
                            },
                            None,
                        )
                        .is_pending()
                );
            }
            scheduled(&f, "observe", &[f.value(&task)])
        } else {
            let execution = f
                .runtime
                .backend()
                .start(&f.module, "value", &[], Default::default())
                .unwrap();
            f.runtime
                .runtime()
                .schedule_execution(execution, decode)
                .unwrap()
        };
        let mut sibling = scheduled(&f, "long", &[]);
        f.runtime
            .runtime()
            .set_execution_observer(RejectEntry(Some(RuntimeError::module_validation(
                "scheduler entry rejected",
            ))))
            .unwrap();
        host.until(&mut f.runtime, || {
            rejected.is_finished() && sibling.is_finished()
        });
        let error = result(&mut rejected).unwrap_err();
        assert_eq!(
            error.message(),
            "module validation failed: scheduler entry rejected"
        );
        assert!(error.trace().is_some());
        if task_entry {
            let Some(TaskEvent::Failed(event)) = f.runtime.take_task_event() else {
                panic!("failed Task retired and reported")
            };
            assert_eq!(
                event.failure.error.message(),
                "module validation failed: scheduler entry rejected"
            );
        } else {
            assert!(!error.trace().unwrap().frames.is_empty());
        }
        assert_eq!(result(&mut sibling).unwrap(), 200);
        assert!(!f.runtime.runtime().is_quarantined());
        assert_eq!(f.cancels.load(Ordering::SeqCst), 0);
        assert_eq!(late.complete(Ok(9)), CompletionStatus::Accepted);
        host.until(&mut f.runtime, || pending.is_finished());
        assert_eq!(result(&mut pending).unwrap(), 9);
    }
}

#[test]
fn scheduler_fatal_entry_failure_drains_other_work() {
    for (task_entry, resume_task) in [(false, false), (true, false), (true, true)] {
        let mut f = Fixture::source(SOURCE);
        let mut pending = scheduled(&f, "io", &[Value::I32(10)]);
        assert!(
            f.runtime
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        let late = f.sent.lock().unwrap().pop().unwrap();
        let scope = f
            .runtime
            .create_task_scope_with_context(&f.module, &Default::default())
            .unwrap();
        let mut rejected = if task_entry {
            let task = f.invoke("busy", &[f.value(scope.capability())]);
            if resume_task {
                let one = NonZeroUsize::new(1).unwrap();
                assert!(
                    f.runtime
                        .poll_scheduler(
                            &mut Context::from_waker(Waker::noop()),
                            TurnBudget {
                                instructions: one,
                                quantum: one,
                                activations: one
                            },
                            None,
                        )
                        .is_pending()
                );
            }
            scheduled(&f, "observe", &[f.value(&task)])
        } else {
            let execution = f
                .runtime
                .backend()
                .start(&f.module, "value", &[], Default::default())
                .unwrap();
            f.runtime
                .runtime()
                .schedule_execution(execution, decode)
                .unwrap()
        };
        f.runtime
            .runtime()
            .set_execution_observer(RejectEntry(Some(RuntimeError::new(
                RuntimeErrorKind::EngineFault,
                "fatal scheduler entry",
            ))))
            .unwrap();
        assert!(matches!(
            f.runtime.poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(_))
        ));
        assert!(f.runtime.runtime().is_quarantined());
        assert!(rejected.is_finished() && pending.is_finished());
        assert_eq!(
            result(&mut pending).unwrap_err().kind(),
            RuntimeErrorKind::EngineFault
        );
        let error = result(&mut rejected).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
        if task_entry {
            let Some(TaskEvent::Failed(event)) = f.runtime.take_task_event() else {
                panic!("fatal Task outcome")
            };
            assert_eq!(event.failure.error.message(), "fatal scheduler entry");
        } else {
            assert_eq!(error.message(), "fatal scheduler entry");
            assert!(!error.trace().unwrap().frames.is_empty());
        }
        assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
        assert_eq!(late.complete(Ok(9)), CompletionStatus::Stale);
    }
}

struct CleanupFault;

impl Drop for CleanupFault {
    fn drop(&mut self) {
        panic!("host completion capture destructor")
    }
}

#[test]
fn scheduler_off_driver_result_drop_fault_wakes_io_driver() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    let execution = f
        .runtime
        .backend()
        .start(&f.module, "value", &[], Default::default())
        .unwrap();
    let completed = f
        .runtime
        .runtime()
        .schedule_execution(execution, |_, _| Ok(CleanupFault))
        .unwrap();
    let mut pending = scheduled(&f, "io", &[Value::I32(10)]);
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    host.until(&mut f.runtime, || {
        completed.is_finished() && f.starts.load(Ordering::SeqCst) == 1
    });
    host.settle(&mut f.runtime);
    let late = f.sent.lock().unwrap().pop().unwrap();
    thread::spawn(move || drop(completed)).join().unwrap();
    host.receive
        .recv_timeout(Duration::from_secs(2))
        .expect("off-driver fault must wake the sleeping runtime");
    host.acknowledge();
    assert!(matches!(
        f.runtime.poll(&mut host.context()),
        Poll::Ready(Err(_))
    ));
    assert!(f.runtime.runtime().is_quarantined());
    assert_eq!(
        result(&mut pending).unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(late.complete(Ok(9)), CompletionStatus::Stale);
}

#[test]
fn scheduler_cleanup_fault_drains_without_later_poll() {
    for entry_failure in [false, true] {
        let mut f = Fixture::source(SOURCE);
        let mut pending = scheduled(&f, "io", &[Value::I32(10)]);
        assert!(
            f.runtime
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        assert_eq!(f.starts.load(Ordering::SeqCst), 1);
        let late = f.sent.lock().unwrap().pop().unwrap();
        let execution = f
            .runtime
            .backend()
            .start(&f.module, "value", &[], Default::default())
            .unwrap();
        let fault = CleanupFault;
        let mut cancelled = f
            .runtime
            .runtime()
            .schedule_execution(execution, move |runtime, value| {
                let _held = &fault;
                decode(runtime, value)
            })
            .unwrap();
        if entry_failure {
            f.runtime
                .runtime()
                .set_execution_observer(RejectEntry(Some(RuntimeError::module_validation(
                    "rejected before cleanup fault",
                ))))
                .unwrap();
            assert!(matches!(
                f.runtime.poll(&mut Context::from_waker(Waker::noop())),
                Poll::Ready(Err(_))
            ));
        } else {
            assert!(
                f.runtime
                    .runtime()
                    .cancel_scheduled_execution(&cancelled)
                    .is_err()
            );
        }
        assert!(f.runtime.runtime().is_quarantined());
        assert!(pending.is_finished() && cancelled.is_finished());
        assert_eq!(
            result(&mut pending).unwrap_err().kind(),
            RuntimeErrorKind::EngineFault
        );
        let error = result(&mut cancelled).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
        assert!(!error.trace().unwrap().frames.is_empty());
        assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
        assert_eq!(late.complete(Ok(1)), CompletionStatus::Stale);
    }
}

#[test]
fn scheduler_conversion_fault_drains_other_invocations() {
    let mut f = Fixture::source(SOURCE);
    let execution = f
        .runtime
        .backend()
        .start(&f.module, "value", &[], Default::default())
        .unwrap();
    let mut broken = f
        .runtime
        .runtime()
        .schedule_execution(execution, |_, _| -> NativeResult<i32> {
            panic!("host conversion failed")
        })
        .unwrap();
    let mut remaining = scheduled(&f, "io", &[Value::I32(10)]);
    assert!(matches!(
        f.runtime.poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(_))
    ));
    assert!(f.runtime.runtime().is_quarantined());
    assert!(broken.is_finished() && remaining.is_finished());
    assert_eq!(
        result(&mut broken).unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        result(&mut remaining).unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
}
