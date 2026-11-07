use super::{BodyCheckGate, install};
use crate::analysis::work::{Event, record};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc::sync_channel,
    thread,
    time::Duration,
};

#[test]
fn gate_observes_actual_entry_once_without_a_measurement_scope() {
    let (gate, started, release) = BodyCheckGate::new();
    let (finished, done) = sync_channel(1);
    let worker = thread::spawn(move || {
        let _scope = install(gate).unwrap();
        assert!(install(BodyCheckGate::new().0).is_err());
        record(Event::SelectedBody);
        record(Event::BodyCheck);
        // The scope remains active after consumption, so nested ownership cannot
        // silently change midway through the operation.
        assert!(install(BodyCheckGate::new().0).is_err());
        record(Event::BodyCheck);
        finished.send(()).unwrap();
    });
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(done.try_recv().is_err());
    // This other thread's entry cannot consume or release the worker's gate.
    record(Event::BodyCheck);
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.join().unwrap();
    assert!(started.try_recv().is_err());
}

#[test]
fn unwind_and_disconnected_observers_release_gate_ownership() {
    let failure = catch_unwind(AssertUnwindSafe(|| {
        let _scope = install(BodyCheckGate::new().0).unwrap();
        panic!("controlled operation unwind");
    }));
    assert!(failure.is_err());
    let (gate, started, release) = BodyCheckGate::new();
    let _scope = install(gate).unwrap();
    drop(release);
    record(Event::BodyCheck);
    started.recv_timeout(Duration::from_secs(2)).unwrap();
}
