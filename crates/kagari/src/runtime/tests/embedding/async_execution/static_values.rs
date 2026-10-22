//! Persistent async values retain code, not a snapshot of mutable module state.
use super::{Fixture, Value, slice};
use crate::runtime::{
    owned::DriveResult, tests::embedding::generic_reload::prepare_static_program,
};
use kagari_runtime::task::drive::TaskDriveResult;
use std::sync::{Arc, atomic::Ordering};

#[test]
fn persistent_static_future_and_task_keep_old_code_but_observe_shared_state() {
    let source = r#"
use test::async_sdk::request;
static var STATE: i32 = 1;
static var SAVED_FUTURE: Option<Future<i32>> = None;
static var SAVED_TASK: Option<Task<i32>> = None;
fn version() -> i32 { 10 }
async fn work() -> i32 {
    val response = request(version()).await;
    STATE += 1;
    version() + STATE + response
}
pub fn save_future() { SAVED_FUTURE = Some(async { work().await }); }
pub fn factory() -> fn()->Future<i32> { || work() }
pub fn save_task(task: Task<i32>) { SAVED_TASK = Some(task); }
pub async fn await_future() -> i32 {
    match SAVED_FUTURE { Some(future) => future.await + version(), None => -1 }
}
pub async fn await_task() -> i32 {
    match SAVED_TASK { Some(task) => task.await + version(), None => -1 }
}
pub fn set_state(value: i32) { STATE = value; }
pub fn state() -> i32 { STATE }
"#;
    for decoded in [false, true] {
        let mut f = Fixture::configured(Default::default());
        f.program = prepare_static_program(&f.engine, source, decoded);
        f.module = f
            .runtime
            .load_program(&f.program, Default::default())
            .unwrap();
        let replacement = prepare_static_program(
            &f.engine,
            &source.replace("fn version() -> i32 { 10 }", "fn version() -> i32 { 20 }"),
            decoded,
        );
        let _ = f.invoke("save_future", &[]);
        let scope = f
            .runtime
            .create_task_scope(&f.module, &Default::default(), Arc::new(|_| Ok(())))
            .unwrap();
        let factory = f.invoke("factory", &[]);
        let task = f
            .runtime
            .runtime()
            .spawn_task(&f.value(scope.capability()), &f.value(&factory))
            .unwrap()
            .unwrap();
        let id = f.runtime.runtime().task_id(&f.value(&task)).unwrap();
        let _ = f.invoke("save_task", &[f.value(&task)]);
        drop((factory, task));
        assert_eq!(
            f.starts.load(Ordering::SeqCst),
            0,
            "storing does not start async work"
        );
        f.module = f
            .runtime
            .reload_program(&f.module, &replacement, Default::default())
            .unwrap();
        let _ = f.invoke("set_state", &[Value::I32(5)]);
        let future = f.invoke("await_future", &[]);
        let owner = f.start_future(&future);
        drop(future);
        assert!(matches!(
            f.runtime.drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap(), vec![10]);
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(20));
        assert_eq!(f.value(&f.complete(&owner)), Value::I32(56));
        assert_eq!(f.value(&f.invoke("state", &[])), Value::I32(6));

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
        assert_eq!(*f.inputs.lock().unwrap(), vec![10, 10]);
        let _ = f.invoke("set_state", &[Value::I32(50)]);
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(30));
        assert_eq!(
            f.runtime.drive_task(&scope, id, slice()).unwrap(),
            TaskDriveResult::Complete
        );
        let report = f
            .runtime
            .runtime()
            .take_task_report(&scope, id)
            .unwrap()
            .unwrap();
        assert_eq!(f.value(&report.outcome.unwrap()), Value::I32(91));
        assert_eq!(f.value(&f.complete(&waiter)), Value::I32(111));
        assert_eq!(f.value(&f.invoke("state", &[])), Value::I32(51));
        assert_eq!(f.starts.load(Ordering::SeqCst), 2);
    }
}
