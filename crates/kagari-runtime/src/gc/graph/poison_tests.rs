use super::{ControlError, GraphIdentity, GraphState, MarkPhase, marks::OwnerMark};
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::allocations;
use crate::{
    error::RuntimeError,
    execution_metadata::scan_records::{NativeApplicationScanId, ScanRecordId},
};
use std::{
    fmt::{self, Write},
    panic::{AssertUnwindSafe, catch_unwind},
};

fn assert_poisoned(state: &GraphState) {
    // Observe before any acquisition can repair a raw mutex's poison state.
    assert_eq!(state.phase(), MarkPhase::Poisoned);
    assert_eq!(
        state.with_access(|| panic!("poison must reject before resolving a mark")),
        Err(ControlError::Poisoned)
    );
    let error = state
        .with_publication(|| panic!("poison must reject before preparing work"))
        .unwrap_err();
    assert_eq!(
        error.message(),
        "module validation failed: poisoned graph control state"
    );
    assert_eq!(state.try_lock().unwrap_err(), ControlError::Poisoned);
    assert_eq!(state.phase(), MarkPhase::Poisoned);
}

#[test]
fn control_rejections_stay_compact_until_reported() {
    let state = GraphState::default();
    let mut guard = state.lock();
    guard.begin_cycle().unwrap();
    guard.finish_roots().unwrap();
    guard.close_mark().unwrap();
    let check = || {
        let lock = state.try_lock().unwrap_err();
        let access = state
            .with_access(|| panic!("borrowed control must reject before mark resolution"))
            .unwrap_err();
        (lock, access)
    };
    #[cfg(feature = "execution-diagnostics")]
    let rejected = {
        let (rejected, counts) = allocations::measure(check);
        assert_eq!(counts.requests, 0);
        rejected
    };
    #[cfg(not(feature = "execution-diagnostics"))]
    let rejected = check();
    assert_eq!(rejected, (ControlError::Borrowed, ControlError::Borrowed));
    let error = RuntimeError::from(rejected.1);
    assert_eq!(
        error.message(),
        "module validation failed: graph control state is borrowed"
    );
    drop(guard);
    assert!(!state.accessible(OwnerMark::default().as_ref()).unwrap());

    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _guard = state.lock();
            panic!("poison before availability-only rejection");
        }))
        .is_err()
    );
    let check = || {
        assert_eq!(state.try_lock().unwrap_err(), ControlError::Poisoned);
        state
            .with_access(|| panic!("poisoned mark resolution"))
            .unwrap_or(false)
    };
    #[cfg(feature = "execution-diagnostics")]
    let available = {
        let (available, counts) = allocations::measure(check);
        assert_eq!(counts.requests, 0);
        available
    };
    #[cfg(not(feature = "execution-diagnostics"))]
    let available = check();
    assert!(!available);
    assert_poisoned(&state);
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
fn recovery_publication_during_existing_unwind_preserves_phase_and_checks() {
    struct PublishOnDrop<'a> {
        state: &'a GraphState,
        target: GraphIdentity,
        mark: &'a OwnerMark,
        result: &'a mut Option<Result<(), RuntimeError>>,
    }

    impl Drop for PublishOnDrop<'_> {
        fn drop(&mut self) {
            *self.result = Some(self.state.shade(self.target, self.mark.as_ref()));
        }
    }

    for phase in [
        MarkPhase::Idle,
        MarkPhase::Roots,
        MarkPhase::Mark,
        MarkPhase::Sweep,
        MarkPhase::Faulted,
    ] {
        let state = GraphState::default();
        let target = GraphIdentity::NativeApplication(NativeApplicationScanId::from_parts(1, 0, 0));
        let mark = OwnerMark::default();
        {
            let mut marker = state.lock();
            marker.prepare_identity().unwrap();
            marker.admit_prepared(target, mark.as_ref());
            if phase != MarkPhase::Idle {
                marker.begin_cycle().unwrap();
            }
            if matches!(phase, MarkPhase::Mark | MarkPhase::Sweep) {
                marker.finish_roots().unwrap();
            }
            if phase == MarkPhase::Sweep {
                // A white target remains condemned: unwind is not permission to
                // resurrect it or to bypass the closed graph's publication check.
                marker.close_mark().unwrap();
            }
            if phase == MarkPhase::Faulted {
                marker.fault();
            }
        }
        let mut publication = None;
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _cleanup = PublishOnDrop {
                    state: &state,
                    target,
                    mark: &mark,
                    result: &mut publication,
                };
                panic!("panic predates recovery graph acquisition");
            }))
            .is_err()
        );
        assert_eq!(
            publication.unwrap().is_ok(),
            !matches!(phase, MarkPhase::Sweep | MarkPhase::Faulted)
        );
        assert!(!state.0.work.is_poisoned());
        assert_eq!(state.phase(), phase);
        assert_eq!(
            state.accessible(mark.as_ref()).unwrap(),
            phase != MarkPhase::Sweep
        );
        let mut marker = state.try_lock().unwrap();
        if matches!(phase, MarkPhase::Roots | MarkPhase::Mark) {
            // Recovery must actually enqueue the target, not merely avoid poison.
            assert_eq!(
                marker
                    .next_owner(|_, _| panic!("singleton fixture"))
                    .unwrap(),
                Some(target)
            );
            marker.finish_owner(target, mark.as_ref()).unwrap();
            if phase == MarkPhase::Roots {
                marker.finish_roots().unwrap();
            }
            marker.close_mark().unwrap();
            assert!(marker.is_survivor(mark.as_ref()).unwrap());
        }
    }
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
