//! Routing races and hostile notification destruction are heap-independent.
use crate::scheduler::wake::WakeHub;
use std::{
    sync::{
        Arc, Barrier, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Wake, Waker},
    thread,
};

#[derive(Default)]
struct Count(AtomicUsize);

impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn scheduler_wake_registration_handoff_and_races() {
    let hub = Arc::new(WakeHub::default());
    let external = Arc::new(Count::default());
    let borrowed = Arc::new(Count::default());
    let replacement = Arc::new(Count::default());
    hub.mark_ready(); // No registration yet; readiness must survive.
    hub.register_external(&Waker::from(external.clone()))
        .unwrap();
    let driver = hub.borrow(&Waker::from(borrowed.clone())).unwrap();
    assert_eq!(borrowed.0.load(Ordering::SeqCst), 1);
    hub.mark_ready();
    assert_eq!(borrowed.0.load(Ordering::SeqCst), 1, "coalesced wake");
    hub.update(&driver, &Waker::from(replacement.clone()))
        .unwrap();
    hub.begin_turn();
    let barrier = Arc::new(Barrier::new(2));
    let worker = {
        let (hub, barrier) = (hub.clone(), barrier.clone());
        thread::spawn(move || {
            barrier.wait();
            hub.mark_ready();
        })
    };
    barrier.wait();
    hub.finish_turn(false);
    worker.join().unwrap();
    assert!(
        replacement.0.load(Ordering::SeqCst) > 0,
        "notification during turn survives"
    );
    drop(driver);
    assert!(
        external.0.load(Ordering::SeqCst) > 0,
        "handoff replays to persistent route"
    );
    hub.begin_turn();
    hub.finish_turn(false);
    let idle = external.0.load(Ordering::SeqCst);
    hub.finish_turn(false);
    assert_eq!(
        external.0.load(Ordering::SeqCst),
        idle,
        "idle does not self-wake"
    );
    hub.close();
    hub.mark_ready();
    assert_eq!(
        external.0.load(Ordering::SeqCst),
        idle,
        "late notifications are inert"
    );
}

#[derive(Default)]
struct HostileWake(AtomicUsize);

impl Wake for HostileWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("host notification failed");
    }
}

#[test]
fn scheduler_fault_notification_does_not_retry_hostile_waker() {
    for fault_first in [false, true] {
        let hub = Arc::new(WakeHub::default());
        let wake = Arc::new(HostileWake::default());
        hub.register_external(&Waker::from(wake.clone())).unwrap();
        if fault_first {
            assert!(
                hub.contain(|| panic!("off-driver cleanup failed"))
                    .is_none()
            );
        } else {
            hub.mark_ready();
        }
        assert!(hub.check().is_err());
        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        hub.mark_ready();
        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        hub.close();
    }
}

struct HostileDrop {
    hub: Weak<WakeHub>,
    dropped: Arc<AtomicUsize>,
}

// Waker::noop cannot exercise destruction of a retained host-owned waker.
#[allow(clippy::manual_noop_waker)]
impl Wake for HostileDrop {
    fn wake(self: Arc<Self>) {}
}

impl Drop for HostileDrop {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
        // Reenter notification routing to prove that destruction is outside locks.
        self.hub.upgrade().unwrap().mark_ready();
        panic!("host waker destructor");
    }
}

#[test]
fn scheduler_wake_destruction_is_contained_once() {
    let hub = Arc::new(WakeHub::default());
    let dropped = Arc::new(AtomicUsize::new(0));
    let wake = Waker::from(Arc::new(HostileDrop {
        hub: Arc::downgrade(&hub),
        dropped: dropped.clone(),
    }));
    hub.register_external(&wake).unwrap();
    drop(wake);
    assert!(hub.register_external(Waker::noop()).is_err());
    hub.close();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}
