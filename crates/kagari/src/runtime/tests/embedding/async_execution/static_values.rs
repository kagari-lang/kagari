//! Persistent async values retain code, not a snapshot of mutable module state.
use super::{Fixture, Value, slice};
use crate::runtime::tests::embedding::generic_reload::prepare_static_program;
use kagari_runtime::task::drive::TaskDriveResult;
use kagari_vm::vm::owned::DriveResult;
use rstest::rstest;
use std::sync::{Arc, atomic::Ordering};

#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_future_and_task_keep_old_code_but_observe_shared_state(#[case] decoded: bool) {
    let source =
        include_str!("../../../../../tests/fixtures/embedding/async/persistent-values.kgr");
    let original_version = "fn version() -> i32 {\n    10\n}";
    assert_eq!(source.matches(original_version).count(), 1);
    let f = Fixture::with_prepared_program(
        Default::default(),
        |_| {},
        |engine, _, _| prepare_static_program(engine, source, decoded),
    );
    let replacement = prepare_static_program(
        &f.engine,
        &source.replace(original_version, "fn version() -> i32 {\n    20\n}"),
        decoded,
    );
    let _ = f.invoke("save_future", &[]);
    let scope = f
        .runtime
        .runtime()
        .create_task_scope(
            &f.runtime.runtime().select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| Ok(())),
        )
        .unwrap();
    let factory = f.invoke("factory", &[]);
    let task = unsafe {
        f.runtime
            .runtime()
            .spawn_task_transport(&f.value(scope.capability()), &f.value(&factory))
    }
    .unwrap()
    .unwrap();
    let id = unsafe { f.runtime.runtime().task_id_transport(&f.value(&task)) }.unwrap();
    let _ = f.invoke("save_task", &[f.value(&task)]);
    drop((factory, task));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "storing does not start async work"
    );
    f.runtime
        .reload_program(&f.module, &replacement, Default::default())
        .unwrap();
    let _ = f.invoke("set_state", &[Value::I32(5)]);
    let future = f.invoke("await_future", &[]);
    let owner = f.start_future(&future);
    drop(future);
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(*f.inputs.lock().unwrap(), vec![20]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(20));
    assert_eq!(f.value(&f.complete(&owner)), Value::I32(66));
    assert_eq!(f.value(&f.invoke("state", &[])), Value::I32(6));

    let waiter = f.invoke("await_task", &[]);
    let waiter = f.start_future(&waiter);
    assert!(matches!(
        f.runtime.backend().drive(&waiter, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.runtime.backend().drive_task(&scope, id, slice()).unwrap(),
        TaskDriveResult::Waiting
    );
    assert_eq!(*f.inputs.lock().unwrap(), vec![20, 20]);
    let _ = f.invoke("set_state", &[Value::I32(50)]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(30));
    assert_eq!(
        f.runtime.backend().drive_task(&scope, id, slice()).unwrap(),
        TaskDriveResult::Complete
    );
    let report = f
        .runtime
        .runtime()
        .take_task_report(&scope, id)
        .unwrap()
        .unwrap();
    assert_eq!(f.value(&report.outcome.unwrap()), Value::I32(101));
    assert_eq!(f.value(&f.complete(&waiter)), Value::I32(121));
    assert_eq!(f.value(&f.invoke("state", &[])), Value::I32(51));
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);
}
