use super::*;
use std::{
    sync::{Barrier, atomic::AtomicUsize},
    task::Wake,
    thread,
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
