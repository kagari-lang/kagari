use super::{Event, Phase, Work, measure, phase, record};
use std::{cell::Cell, panic::catch_unwind};

#[test]
fn nested_scope_rejects_without_stealing_counts_and_unwind_releases_scope() {
    let invoked = Cell::new(false);
    let outer = measure(|| {
        record(Event::BodyCheck);
        assert!(measure(|| invoked.set(true)).is_err());
        record(Event::BodyRestore);
        Err::<(), _>("operation failed")
    })
    .unwrap();
    assert!(!invoked.get());
    assert_eq!(outer.value, Err("operation failed"));
    assert_eq!(outer.work.body_checks_started, 1);
    assert_eq!(outer.work.bodies_restored, 1);
    assert!(
        catch_unwind(|| {
            let _ = measure(|| {
                let _phase = phase(Phase::BodyCheck);
                record(Event::BodyCheck);
                panic!("operation unwound");
            });
        })
        .is_err()
    );
    assert_eq!(measure(|| ()).unwrap().work, Work::default());
}

#[test]
fn counters_are_thread_scoped_and_escaped_timers_cannot_charge_another_operation() {
    let previous = measure(|| {
        record(Event::SelectedBody);
        phase(Phase::BodyCheck)
    })
    .unwrap();
    assert_eq!(previous.work.selected_body_calls, 1);
    assert_eq!(previous.work.phase_nanoseconds(Phase::BodyCheck), 0);
    let current = measure(|| {
        drop(previous.value);
        std::thread::spawn(|| record(Event::FullSnapshot))
            .join()
            .unwrap();
        record(Event::SharedBody);
    })
    .unwrap();
    assert_eq!(
        current.work,
        Work {
            shared_body_hits: 1,
            ..Work::default()
        }
    );
}
