//! Persistent async values retain code, not a snapshot of mutable module state.
use super::{Fixture, RootedValue, slice};
use crate::runtime::tests::embedding::generic_reload::prepare_static_program;
use kagari_runtime::execution::owned::DriveResult;
use kagari_runtime::reload::ReloadValidationError;
use kagari_runtime::task::drive::TaskDriveResult;
use rstest::rstest;
use std::slice as slices;
use std::sync::{Arc, atomic::Ordering};

#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_future_and_task_keep_old_code_but_observe_shared_state(#[case] decoded: bool) {
    let source =
        include_str!("../../../../../tests/fixtures/embedding/async/persistent-values.kgr");
    let original_version = "fn version() -> i32 {\n    10\n}";
    assert_eq!(source.matches(original_version).count(), 1);
    let mut f = Fixture::with_prepared_program(
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
        .create_dispatched_task_scope(
            &f.runtime.select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| Ok(())),
        )
        .unwrap();
    let factory = f.invoke("factory", &[]);
    let task = f
        .runtime
        .spawn_task(&(scope.capability()).clone(), &factory)
        .unwrap()
        .unwrap();
    let id = f.runtime.task_id(&task).unwrap();
    let _ = f.invoke("save_task", slices::from_ref(&task));
    drop((factory, task));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "storing does not start async work"
    );
    f.runtime
        .program(&f.module)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(replacement.program(), &Default::default()))
        .and_then(|candidate| {
            f.runtime
                .replace_program(&f.module, &candidate, Default::default())
        })
        .unwrap();
    let _ = f.invoke("set_state", &[RootedValue::from(5_i32)]);
    let future = f.invoke("await_future", &[]);
    let owner = f.start_future(&future);
    drop(future);
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(*f.inputs.lock().unwrap(), vec![20]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(20));
    assert_eq!(f.complete(&owner).as_i32().unwrap(), 66);
    assert_eq!(f.invoke("state", &[]).as_i32().unwrap(), 6);

    let waiter = f.invoke("await_task", &[]);
    let waiter = f.start_future(&waiter);
    assert!(matches!(
        f.runtime.drive(&waiter, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.runtime.drive_task(&scope, id, slice()).unwrap(),
        TaskDriveResult::Waiting
    );
    assert_eq!(*f.inputs.lock().unwrap(), vec![20, 20]);
    let _ = f.invoke("set_state", &[RootedValue::from(50_i32)]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(30));
    assert_eq!(
        f.runtime.drive_task(&scope, id, slice()).unwrap(),
        TaskDriveResult::Complete
    );
    let report = f.runtime.take_task_report(&scope, id).unwrap().unwrap();
    assert_eq!(report.outcome.unwrap().as_i32().unwrap(), 101);
    assert_eq!(f.complete(&waiter).as_i32().unwrap(), 121);
    assert_eq!(f.invoke("state", &[]).as_i32().unwrap(), 51);
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);
}
