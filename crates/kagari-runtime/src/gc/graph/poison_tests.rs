use super::{GraphState, MarkPhase};
use std::{
    fmt::{self, Write},
    panic::{AssertUnwindSafe, catch_unwind},
};

fn assert_poisoned(state: &GraphState) {
    // Observe before any acquisition can repair a raw mutex's poison state.
    assert_eq!(state.phase(), MarkPhase::Poisoned);
    assert!(
        state
            .with_access(|| panic!("poison must reject before resolving a mark"))
            .is_err()
    );
    assert!(
        state
            .with_publication(|| panic!("poison must reject before preparing work"))
            .is_err()
    );
    assert!(state.try_lock().is_err());
    assert_eq!(state.phase(), MarkPhase::Poisoned);
}

#[test]
fn guard_unwind_publishes_terminal_poison_from_every_phase() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<GraphState>();
    for phase in [
        MarkPhase::Idle,
        MarkPhase::Roots,
        MarkPhase::Mark,
        MarkPhase::Sweep,
        MarkPhase::Faulted,
    ] {
        let state = GraphState::default();
        let observer = state.clone();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let mut marker = state.lock();
                if phase != MarkPhase::Idle {
                    marker.begin_cycle().unwrap();
                }
                if matches!(phase, MarkPhase::Mark | MarkPhase::Sweep) {
                    marker.finish_roots().unwrap();
                }
                if phase == MarkPhase::Sweep {
                    marker.close_mark().unwrap();
                }
                if phase == MarkPhase::Faulted {
                    marker.fault();
                }
                assert_eq!(observer.phase(), phase);
                panic!("injected guarded unwind");
            }))
            .is_err()
        );
        assert_poisoned(&observer);
        assert!(state.0.work.is_poisoned());
    }
}

#[test]
fn acquisition_during_unwind_poison_is_authoritative_without_mutex_poison() {
    struct AcquireOnDrop<'a>(&'a GraphState);

    impl Drop for AcquireOnDrop<'_> {
        fn drop(&mut self) {
            let marker = self.0.try_lock().unwrap();
            assert_eq!(marker.phase(), MarkPhase::Idle);
            drop(marker);
        }
    }

    let state = GraphState::default();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _cleanup = AcquireOnDrop(&state);
            panic!("panic predates graph acquisition");
        }))
        .is_err()
    );
    // std records the panic state at acquisition and does not poison this case.
    // Graph work nevertheless must not become usable after our unwinding drop.
    assert!(!state.0.work.is_poisoned());
    assert_poisoned(&state);
}

#[test]
fn caught_panic_without_guard_unwind_does_not_poison_graph_work() {
    let state = GraphState::default();
    assert!(catch_unwind(|| panic!("before acquisition")).is_err());
    let marker = state.lock();
    assert!(catch_unwind(|| panic!("caught without dropping the guard")).is_err());
    assert_eq!(marker.phase(), MarkPhase::Idle);
    drop(marker);
    assert!(!state.0.work.is_poisoned());
    assert_eq!(state.phase(), MarkPhase::Idle);
    state
        .with_publication(|| panic!("Idle needs no work"))
        .unwrap();
}

#[test]
fn debug_formatter_unwind_uses_the_same_poison_owner() {
    struct PanicInWork;

    impl Write for PanicInWork {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            assert_ne!(text, "GraphWork", "panic while formatting locked work");
            Ok(())
        }
    }

    let state = GraphState::default();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            write!(&mut PanicInWork, "{state:?}").unwrap();
        }))
        .is_err()
    );
    assert_poisoned(&state);
    assert!(state.0.work.is_poisoned());
    // Poisoned and already borrowed Debug paths must not unwrap or deadlock.
    assert!(format!("{state:?}").contains("Poisoned"));
    let healthy = GraphState::default();
    let _guard = healthy.lock();
    assert!(format!("{healthy:?}").contains("unavailable"));
}

#[test]
fn acquisition_defensively_publishes_bypassed_mutex_poison() {
    let state = GraphState::default();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            // Deliberately violate the private wrapping rule to test the defensive
            // acquisition branch. No production path may obtain this raw guard.
            let _raw = state.0.work.try_lock().unwrap();
            panic!("injected raw mutex poison");
        }))
        .is_err()
    );
    assert_eq!(state.phase(), MarkPhase::Idle);
    assert!(state.0.work.is_poisoned());
    assert!(state.try_lock().is_err());
    assert_poisoned(&state);
}
