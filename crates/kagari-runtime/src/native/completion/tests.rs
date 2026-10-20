use super::*;
use std::{
    panic::panic_any,
    sync::{
        Barrier,
        atomic::AtomicUsize,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    task::Wake,
    thread,
    time::Duration,
};

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
struct Tracked(Arc<AtomicUsize>);

impl Drop for Tracked {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn async_native_completion_contract_reservation_delivery_and_retirement() {
    let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
    let mut operation = registry.reserve::<String>().unwrap();
    let completion = operation.completion().unwrap();
    assert_eq!(registry.active_count(), 1);
    assert_eq!(
        registry.reserve::<String>().unwrap_err().kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    assert!(matches!(operation.poll().unwrap(), OperationPoll::Pending));
    // A provider may complete synchronously before returning its cancellation hook.
    assert_eq!(
        completion.complete(Ok("first".into())),
        CompletionStatus::Accepted
    );
    let cancels = Arc::new(AtomicUsize::new(0));
    let cancelled = cancels.clone();
    operation
        .on_cancel(move || {
            cancelled.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();
    let wakes = Arc::new(WakeCount::default());
    operation.set_waker(&Waker::from(wakes.clone()));
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(
        completion.complete(Ok("duplicate".into())),
        CompletionStatus::Duplicate
    );
    let OperationPoll::Ready(Ok(value)) = operation.poll().unwrap() else {
        panic!("ready result")
    };
    assert_eq!(value, "first");
    assert_eq!(registry.active_count(), 0);
    assert!(matches!(operation.poll().unwrap(), OperationPoll::Retired));
    assert_eq!(
        completion.complete(Ok("late".into())),
        CompletionStatus::Stale
    );
    drop(operation);
    assert_eq!(
        cancels.load(Ordering::Relaxed),
        0,
        "successful claim disarms cancellation"
    );

    let mut next = registry.reserve::<Result<i32, &'static str>>().unwrap();
    let next_completion = next.completion().unwrap();
    assert_eq!(next_completion.id().slot, completion.id().slot);
    assert_ne!(next_completion.id(), completion.id());
    assert_eq!(
        completion.complete(Ok("old generation".into())),
        CompletionStatus::Stale
    );
    assert!(matches!(next.poll().unwrap(), OperationPoll::Pending));
    next_completion.complete(Ok(Err("business error")));
    assert!(matches!(
        next.poll().unwrap(),
        OperationPoll::Ready(Ok(Err("business error")))
    ));

    let mut failure = registry.reserve::<i32>().unwrap();
    failure
        .completion()
        .unwrap()
        .complete(Err(RuntimeError::new(
            RuntimeErrorKind::HostCallFailure,
            "provider failed",
        )));
    let OperationPoll::Ready(Err(error)) = failure.poll().unwrap() else {
        panic!("provider failure")
    };
    assert_eq!(error.kind(), RuntimeErrorKind::HostCallFailure);

    let dropped = Arc::new(AtomicUsize::new(0));
    let mut cancelled = registry.reserve::<Tracked>().unwrap();
    let late = cancelled.completion().unwrap();
    late.complete(Ok(Tracked(dropped.clone())));
    let count = cancels.clone();
    cancelled
        .on_cancel(move || {
            count.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();
    // Readiness is not a claimed result. Cancellation disposes queued data.
    cancelled.cancel().unwrap();
    cancelled.cancel().unwrap();
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
    assert_eq!(cancels.load(Ordering::Relaxed), 1);
    assert_eq!(
        late.complete(Ok(Tracked(dropped.clone()))),
        CompletionStatus::Stale
    );
    assert_eq!(dropped.load(Ordering::Relaxed), 2);
    assert_eq!(registry.active_count(), 0);
}

#[test]
fn async_native_completion_contract_cross_thread_races() {
    let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
    for _ in 0..16 {
        let mut operation = registry.reserve::<i32>().unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let completions = [11, 22].map(|value| {
            let completion = operation.completion().unwrap();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                (value, completion.complete(Ok(value)))
            })
        });
        let wakes = Arc::new(WakeCount::default());
        barrier.wait();
        operation.set_waker(&Waker::from(wakes.clone()));
        let results = completions.map(|thread| thread.join().unwrap());
        assert_eq!(
            results
                .iter()
                .filter(|(_, status)| *status == CompletionStatus::Accepted)
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|(_, status)| *status == CompletionStatus::Duplicate)
                .count(),
            1
        );
        let expected = results
            .iter()
            .find(|(_, status)| *status == CompletionStatus::Accepted)
            .unwrap()
            .0;
        let OperationPoll::Ready(Ok(actual)) = operation.poll().unwrap() else {
            panic!("race result")
        };
        assert_eq!(actual, expected);
        assert!(
            wakes.0.load(Ordering::Relaxed) >= 1,
            "registration/publication must not lose readiness"
        );
        assert_eq!(registry.active_count(), 0);

        let mut operation = registry.reserve::<i32>().unwrap();
        let completion = operation.completion().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let publishing = barrier.clone();
        let thread = thread::spawn(move || {
            publishing.wait();
            completion.complete(Ok(42))
        });
        barrier.wait();
        operation.cancel().unwrap();
        assert!(matches!(
            thread.join().unwrap(),
            CompletionStatus::Accepted | CompletionStatus::Stale
        ));
        assert!(matches!(operation.poll().unwrap(), OperationPoll::Retired));
        assert_eq!(registry.active_count(), 0);
    }
}

#[derive(Debug)]
struct PanickingDrop(Arc<AtomicUsize>);

impl Drop for PanickingDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("completion cleanup destruction");
    }
}

#[expect(
    clippy::manual_noop_waker,
    reason = "The owned waker's destructor is the failure under test"
)]
impl Wake for PanickingDrop {
    fn wake(self: Arc<Self>) {}
}

fn assert_faulted_retirement<T: Send + 'static>(
    registry: &CompletionRegistry,
    mut operation: Operation<T>,
    id: OperationId,
) {
    assert_eq!(
        registry.check().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(registry.active_count(), 0);
    assert_eq!(
        registry.reserve::<()>().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        operation.cancel().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        operation.poll().err().unwrap().kind(),
        RuntimeErrorKind::EngineFault
    );
    drop(operation);
    let slots = registry.inner.slots.lock().unwrap();
    assert!(!slots[id.slot].active);
    assert_eq!(slots[id.slot].generation, id.generation.checked_add(1));
}

#[test]
fn async_native_completion_contract_queued_payload_cleanup_panic() {
    let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
    let mut operation = registry.reserve::<PanickingDrop>().unwrap();
    let completion = operation.completion().unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let hooks = Arc::new(AtomicUsize::new(0));
    let invoked = hooks.clone();
    let state = operation.state.clone();
    let inner = registry.inner.clone();
    operation
        .on_cancel(move || {
            assert!(state.try_lock().is_ok(), "hook outside publication lock");
            assert!(inner.slots.try_lock().is_ok(), "hook outside slot lock");
            invoked.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();
    completion.complete(Ok(PanickingDrop(drops.clone())));
    let result = catch_unwind(AssertUnwindSafe(|| operation.cancel()));
    assert!(
        result.is_ok(),
        "panic_escaped=true active={} registry_ok={} hooks={}",
        registry.active_count(),
        registry.check().is_ok(),
        hooks.load(Ordering::Relaxed)
    );
    assert_eq!(
        result.unwrap().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(registry.active_count(), 0);
    assert_eq!(hooks.load(Ordering::Relaxed), 1);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_faulted_retirement(&registry, operation, completion.id());
    assert_eq!(hooks.load(Ordering::Relaxed), 1);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(
        completion.complete(Err(RuntimeError::module_validation("late"))),
        CompletionStatus::Stale
    );
}

#[test]
fn async_native_completion_contract_terminal_destructors_are_independent() {
    for claim in [false, true] {
        for (panic_hook, panic_waker) in [(true, false), (false, true), (true, true)] {
            let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
            let mut operation = registry.reserve::<PanickingDrop>().unwrap();
            let completion = operation.completion().unwrap();
            let payload_drops = Arc::new(AtomicUsize::new(0));
            let hook_drops = Arc::new(AtomicUsize::new(0));
            let hook_calls = Arc::new(AtomicUsize::new(0));
            let waker_drops = Arc::new(AtomicUsize::new(0));
            let capture = panic_hook.then(|| PanickingDrop(hook_drops.clone()));
            let calls = hook_calls.clone();
            operation
                .on_cancel(move || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    drop(capture);
                })
                .unwrap();
            if panic_waker {
                let waker = Waker::from(Arc::new(PanickingDrop(waker_drops.clone())));
                operation.set_waker(&waker);
                drop(waker);
            }
            completion.complete(Ok(PanickingDrop(payload_drops.clone())));
            let result = catch_unwind(AssertUnwindSafe(|| {
                if claim {
                    operation.poll().map(|_| ())
                } else {
                    operation.cancel()
                }
            }));
            assert_eq!(
                result.unwrap().unwrap_err().kind(),
                RuntimeErrorKind::EngineFault
            );
            assert_faulted_retirement(&registry, operation, completion.id());
            assert_eq!(payload_drops.load(Ordering::Relaxed), 1);
            assert_eq!(hook_drops.load(Ordering::Relaxed), usize::from(panic_hook));
            assert_eq!(hook_calls.load(Ordering::Relaxed), usize::from(!claim));
            assert_eq!(
                waker_drops.load(Ordering::Relaxed),
                usize::from(panic_waker)
            );
        }
    }
}

struct PanickingWake;

impl Wake for PanickingWake {
    fn wake(self: Arc<Self>) {
        panic!("provider completion wake");
    }
}

#[test]
fn async_native_completion_contract_first_hook_survives_submission_fault() {
    for explicit_cancel in [false, true] {
        for fault_same_operation in [false, true] {
            let registry = CompletionRegistry::new(NonZeroUsize::new(2).unwrap()).unwrap();
            let mut operation = registry.reserve::<()>().unwrap();
            let completion = operation.completion().unwrap();
            let other = (!fault_same_operation).then(|| registry.reserve::<()>().unwrap());
            let publisher = other.as_ref().unwrap_or(&operation);
            publisher.set_waker(&Waker::from(Arc::new(PanickingWake)));
            assert_eq!(
                publisher.completion().unwrap().complete(Ok(())),
                CompletionStatus::Accepted
            );
            assert_eq!(
                registry.check().unwrap_err().kind(),
                RuntimeErrorKind::EngineFault
            );
            drop(other);

            // Submission already happened: returning a fault must not discard
            // the provider's first cancellation obligation, ready or pending.
            let calls = Arc::new(AtomicUsize::new(0));
            let invoked = calls.clone();
            let drops = Arc::new(AtomicUsize::new(0));
            let capture = Tracked(drops.clone());
            assert_eq!(
                operation
                    .on_cancel(move || {
                        invoked.fetch_add(1, Ordering::Relaxed);
                        drop(capture);
                    })
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::EngineFault
            );
            assert_eq!(calls.load(Ordering::Relaxed), 0);
            assert_eq!(
                drops.load(Ordering::Relaxed),
                0,
                "first hook must remain installed"
            );
            assert_eq!(registry.active_count(), 1);
            let rejected_calls = Arc::new(AtomicUsize::new(0));
            let rejected = rejected_calls.clone();
            assert_eq!(
                operation
                    .on_cancel(move || {
                        rejected.fetch_add(1, Ordering::Relaxed);
                    })
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::EngineFault
            );
            if explicit_cancel {
                assert_eq!(
                    operation.cancel().unwrap_err().kind(),
                    RuntimeErrorKind::EngineFault
                );
                assert_faulted_retirement(&registry, operation, completion.id());
            } else {
                drop(operation);
            }
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert_eq!(drops.load(Ordering::Relaxed), 1);
            assert_eq!(rejected_calls.load(Ordering::Relaxed), 0);
            assert_eq!(registry.active_count(), 0);
            assert_eq!(completion.complete(Ok(())), CompletionStatus::Stale);
            let slots = registry.inner.slots.lock().unwrap();
            assert_eq!(slots[completion.id().slot].generation, Some(1));
        }
    }
}

#[test]
fn async_native_completion_contract_rejected_hook_capture_cleanup() {
    for retired in [false, true] {
        let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
        let mut operation = registry.reserve::<()>().unwrap();
        let id = operation.completion().unwrap().id();
        let calls = Arc::new(AtomicUsize::new(0));
        let invoked = calls.clone();
        operation
            .on_cancel(move || {
                invoked.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();
        if retired {
            operation.cancel().unwrap();
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let capture = PanickingDrop(drops.clone());
        let rejected_calls = Arc::new(AtomicUsize::new(0));
        let rejected = rejected_calls.clone();
        assert_eq!(
            operation
                .on_cancel(move || {
                    rejected.fetch_add(1, Ordering::Relaxed);
                    drop(capture);
                })
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert_eq!(rejected_calls.load(Ordering::Relaxed), 0);
        assert_eq!(registry.active_count(), usize::from(!retired));
        assert_eq!(
            operation.cancel().unwrap_err().kind(),
            RuntimeErrorKind::EngineFault
        );
        assert_faulted_retirement(&registry, operation, id);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn async_native_completion_contract_replaced_waker_cleanup() {
    let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
    let mut operation = registry.reserve::<()>().unwrap();
    let id = operation.completion().unwrap().id();
    let drops = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(PanickingDrop(drops.clone())));
    operation.set_waker(&waker);
    drop(waker);
    let wakes = Arc::new(WakeCount::default());
    operation.set_waker(&Waker::from(wakes.clone()));
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(
        registry.check().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        operation.cancel().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_faulted_retirement(&registry, operation, id);
    assert_eq!(Arc::strong_count(&wakes), 1);
}

struct PanicPayload(Arc<AtomicUsize>);

impl Drop for PanicPayload {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic_any(PanicPayload(self.0.clone()));
    }
}

#[test]
fn async_native_completion_contract_panic_payload_destruction_is_contained() {
    let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
    let mut operation = registry.reserve::<()>().unwrap();
    let id = operation.completion().unwrap().id();
    let drops = Arc::new(AtomicUsize::new(0));
    let payload = PanicPayload(drops.clone());
    operation.on_cancel(move || panic_any(payload)).unwrap();
    assert_eq!(
        operation.cancel().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_faulted_retirement(&registry, operation, id);
    assert_eq!(
        drops.load(Ordering::Relaxed),
        1,
        "secondary panic payload is not destroyed"
    );
}

struct PublishingWake {
    entered: SyncSender<()>,
    release: Mutex<Receiver<()>>,
    _destroyed: PanickingDrop,
}

impl Wake for PublishingWake {
    fn wake(self: Arc<Self>) {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }
}

#[test]
fn async_native_completion_contract_publisher_releases_final_waker() {
    let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
    let mut operation = registry.reserve::<()>().unwrap();
    let completion = operation.completion().unwrap();
    let id = completion.id();
    let (enter, entered) = sync_channel(1);
    let (release, released) = sync_channel(1);
    let drops = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(PublishingWake {
        entered: enter,
        release: Mutex::new(released),
        _destroyed: PanickingDrop(drops.clone()),
    }));
    operation.set_waker(&waker);
    drop(waker);
    let producer = thread::spawn(move || completion.complete(Ok(())));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    operation.cancel().unwrap();
    assert_eq!(registry.active_count(), 0);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    release.send(()).unwrap();
    assert_eq!(producer.join().unwrap(), CompletionStatus::Accepted);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_faulted_retirement(&registry, operation, id);
}

#[test]
fn async_native_completion_contract_cleanup_fault_and_generation_exhaustion() {
    let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
    let mut operation = registry.reserve::<i32>().unwrap();
    let completion = operation.completion().unwrap();
    operation
        .on_cancel(|| panic!("invalid provider cleanup"))
        .unwrap();
    assert_eq!(
        operation.cancel().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(completion.complete(Ok(1)), CompletionStatus::Stale);
    assert_eq!(registry.active_count(), 0);
    assert_eq!(
        registry.reserve::<i32>().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );

    let registry = CompletionRegistry::new(NonZeroUsize::new(1).unwrap()).unwrap();
    drop(registry.reserve::<i32>().unwrap());
    registry.inner.slots.lock().unwrap()[0].generation = Some(u64::MAX);
    let operation = registry.reserve::<i32>().unwrap();
    let exhausted = operation.completion().unwrap();
    drop(operation);
    assert_eq!(exhausted.complete(Ok(2)), CompletionStatus::Stale);
    assert_eq!(registry.active_count(), 0);
    assert_eq!(
        registry.reserve::<i32>().unwrap_err().kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
}
