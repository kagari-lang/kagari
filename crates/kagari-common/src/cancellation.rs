use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::Waker,
};

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    subscribers: Mutex<Vec<Weak<WakeRegistration>>>,
}

#[derive(Debug)]
struct WakeRegistration(Waker);

/// Keep a cancellation wake registered for the lifetime of an operation.
/// The token retains only a weak registration, so completed work is not kept alive.
#[derive(Debug)]
pub struct CancellationSubscription {
    _registration: Arc<WakeRegistration>,
}

/// Shared cooperative cancellation for analysis and execution scopes.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<CancellationState>);

impl PartialEq for CancellationToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CancellationToken {}

impl CancellationToken {
    pub fn cancel(&self) {
        if self.0.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        let subscribers = {
            let mut entries = self.0.subscribers.lock().unwrap_or_else(|e| e.into_inner());
            let subscribers = entries.iter().filter_map(Weak::upgrade).collect::<Vec<_>>();
            entries.clear();
            subscribers
        };
        // A host waker may run arbitrary host code. Never invoke it under a lock.
        for subscriber in subscribers {
            subscriber.0.wake_by_ref();
        }
    }

    pub fn check(&self) -> Result<(), Cancelled> {
        if self.0.cancelled.load(Ordering::Acquire) {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }

    /// Register before waiting. Cancellation racing with registration cannot be
    /// lost; duplicate wakeups are permitted and never resume work directly.
    pub fn subscribe(&self, waker: Waker) -> CancellationSubscription {
        let registration = Arc::new(WakeRegistration(waker));
        {
            let mut subscribers = self.0.subscribers.lock().unwrap_or_else(|e| e.into_inner());
            subscribers.retain(|entry| entry.strong_count() != 0);
            subscribers.push(Arc::downgrade(&registration));
        }
        if self.check().is_err() {
            registration.0.wake_by_ref();
        }
        CancellationSubscription {
            _registration: registration,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::atomic::AtomicUsize, task::Wake};

    #[derive(Default)]
    struct Counter(AtomicUsize);

    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn cancellation_subscription_covers_registration_order_and_retirement() {
        let token = CancellationToken::default();
        let retired = Arc::new(Counter::default());
        drop(token.subscribe(Waker::from(retired.clone())));
        let live = Arc::new(Counter::default());
        let _subscription = token.subscribe(Waker::from(live.clone()));
        token.cancel();
        token.cancel();
        assert_eq!(retired.0.load(Ordering::Relaxed), 0);
        assert_eq!(live.0.load(Ordering::Relaxed), 1);
        let late = Arc::new(Counter::default());
        let _late = token.subscribe(Waker::from(late.clone()));
        assert_eq!(late.0.load(Ordering::Relaxed), 1);
        assert!(token.check().is_err());
    }
}
