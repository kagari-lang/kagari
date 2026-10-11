//! Ordinary registered task methods across the synchronous-handler boundary.
use super::{Fixture, Value};
use kagari_embed::engine::EngineConfig;
use kagari_runtime::{
    gc::roots::RootedValue,
    task::{
        CancellationCause, TaskId,
        control::{DispatchUnavailable, TaskScopeOwner},
        drive::TaskDriveResult,
    },
};
use std::{
    num::NonZeroUsize,
    slice,
    sync::{Arc, atomic::Ordering},
};

const SOURCE: &str = r#"
use test::async_sdk::request;
fn forward<T, F: Fn() -> Future<T>>(scope: TaskScope, factory: F) -> Result<Task<T>, SpawnError> {
    scope.spawn(factory)
}
struct Fetch { val events: Vec<i32> }
impl Fn<()> for Fetch {
    type Output = Future<i32>;
    fn call(self, args: ()) -> Future<i32> { self.events.push(3); request(0) }
}
fn items() -> Vec<i32> { Vec::from([10,20]) }
fn events() -> Vec<i32> { Vec::from([]) }
fn len(items: Vec<i32>) -> usize { items.len() }
fn at(items: Vec<i32>, index: usize) -> i32 { items[index] }
fn replace(items: Vec<i32>) { items[1]=40; }
fn append(items: Vec<i32>) { items.push(50); }
fn handler(scope: TaskScope, items: Vec<i32>, events: Vec<i32>) -> Result<Task<i32>, SpawnError> {
    val result = forward(scope, || {
        events.push(1);
        val resume = async || { var sum=0; for item in items { sum += request(item).await; } sum };
        resume()
    });
    events.push(0);
    result
}
fn observer(scope: TaskScope, task: Task<i32>) -> Result<Task<i32>, SpawnError> {
    scope.spawn(async || task.await + 1)
}
fn object(scope: TaskScope, events: Vec<i32>) -> Result<Task<i32>, SpawnError> { forward(scope, Fetch { events }) }
fn nested(scope: TaskScope) -> Result<Task<Future<i32>>, SpawnError> { scope.spawn(async || request(0)) }
fn stop<T>(task: Task<T>) { task.cancel(); }
fn stop_i32(task: Task<i32>) { stop(task); }
fn after_spawn_trap(scope: TaskScope, events: Vec<i32>) {
    val admitted = scope.spawn(async || { events.push(9); 9 });
    val zero=0; val bad=1/zero;
}
fn unwrap<T>(result: Result<Task<T>, SpawnError>) -> Task<T> { match result { Ok(task) => task, Err(_) => { val zero=0; val bad=1/zero; unwrap(result) } } }
fn unwrap_i32(result: Result<Task<i32>, SpawnError>) -> Task<i32> { unwrap(result) }
fn unwrap_nested(result: Result<Task<Future<i32>>, SpawnError>) -> Task<Future<i32>> { unwrap(result) }
fn status(result: Result<Task<i32>, SpawnError>) -> i32 {
    match result { Ok(_) => 0, Err(error) => match error {
        SpawnError::ScopeClosed => 1, SpawnError::CapacityExceeded => 2, SpawnError::DispatchUnavailable => 3
    } }
}
"#;

fn drive(f: &Fixture, scope: &TaskScopeOwner, id: TaskId) -> TaskDriveResult {
    for _ in 0..1000 {
        f.runtime.runtime().collect_garbage().unwrap();
        let result = f
            .runtime
            .drive_task(scope, id, NonZeroUsize::new(1).unwrap())
            .unwrap();
        if result != TaskDriveResult::Runnable {
            return result;
        }
    }
    panic!("bounded task driver");
}

fn task(f: &Fixture, result: &RootedValue) -> (TaskId, RootedValue) {
    let handle = f.invoke("unwrap_i32", &[f.value(result)]);
    (
        f.runtime.runtime().task_id(&f.value(&handle)).unwrap(),
        handle,
    )
}

#[test]
fn scoped_task_handler_contract() {
    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_tasks = NonZeroUsize::new(2).unwrap();
    let f = Fixture::configured_source(SOURCE, config);
    let runtime = f.runtime.runtime();
    let scope = f
        .runtime
        .create_task_scope(&f.module, &Default::default(), Arc::new(|_| Ok(())))
        .unwrap();
    let capability = f.value(scope.capability());
    let events = f.invoke("events", &[]);
    let items = f.invoke("items", &[]);
    let result = f.invoke("handler", &[capability, f.value(&items), f.value(&events)]);
    let (id, handle) = task(&f, &result);
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.value(&f.invoke("len", &[f.value(&events)])),
        Value::U64(1)
    );
    assert_eq!(
        f.value(&f.invoke("at", &[f.value(&events), Value::U64(0)])),
        Value::I32(0)
    );
    assert_eq!(drive(&f, &scope, id), TaskDriveResult::Waiting);
    assert_eq!(f.inputs.lock().unwrap().as_slice(), &[10]);
    assert_eq!(
        f.value(&f.invoke("len", &[f.value(&events)])),
        Value::U64(2)
    );
    assert!(f.call("append", &[f.value(&items)]).is_err());
    let _ = f.invoke("replace", &[f.value(&items)]);
    // A separate synchronous handler captures the running Task and submits a waiter.
    let observer = f.invoke("observer", &[capability, f.value(&handle)]);
    let (waiter, waiter_handle) = task(&f, &observer);
    assert_eq!(drive(&f, &scope, waiter), TaskDriveResult::Waiting);
    let excess = f.invoke("object", &[capability, f.value(&events)]);
    assert_eq!(
        f.value(&f.invoke("status", &[f.value(&excess)])),
        Value::I32(2)
    );
    assert_eq!(
        f.value(&f.invoke("len", &[f.value(&events)])),
        Value::U64(2)
    );
    let _ = f.invoke("stop_i32", &[f.value(&waiter_handle)]);
    assert_eq!(runtime.drain_cancelled_tasks().unwrap(), 1);
    assert_eq!(
        runtime
            .take_task_report(&scope, waiter)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap_err()
            .cancellation,
        Some(CancellationCause::Explicit)
    );
    assert_eq!(f.cancels.load(Ordering::SeqCst), 0);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(5));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "completion is readiness only"
    );
    assert_eq!(drive(&f, &scope, id), TaskDriveResult::Waiting);
    assert_eq!(f.inputs.lock().unwrap().as_slice(), &[10, 40]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(6));
    assert_eq!(drive(&f, &scope, id), TaskDriveResult::Complete);
    let report = runtime.take_task_report(&scope, id).unwrap().unwrap();
    assert_eq!(f.value(&report.outcome.unwrap()), Value::I32(11));
    let _ = f.invoke("append", &[f.value(&items)]);
    let object = f.invoke("object", &[capability, f.value(&events)]);
    let (object_id, _) = task(&f, &object);
    assert_eq!(
        f.value(&f.invoke("len", &[f.value(&events)])),
        Value::U64(2)
    );
    assert_eq!(drive(&f, &scope, object_id), TaskDriveResult::Complete);
    assert_eq!(
        f.value(
            &runtime
                .take_task_report(&scope, object_id)
                .unwrap()
                .unwrap()
                .outcome
                .unwrap()
        ),
        Value::I32(7)
    );
    assert_eq!(
        f.value(&f.invoke("len", &[f.value(&events)])),
        Value::U64(3)
    );
    let nested = f.invoke("nested", slice::from_ref(&capability));
    let nested_handle = f.invoke("unwrap_nested", &[f.value(&nested)]);
    let nested_id = runtime.task_id(&f.value(&nested_handle)).unwrap();
    let starts = f.starts.load(Ordering::SeqCst);
    assert_eq!(drive(&f, &scope, nested_id), TaskDriveResult::Complete);
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        starts,
        "Task<Future<T>> is not flattened"
    );
    let future = runtime
        .take_task_report(&scope, nested_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap();
    assert_eq!(
        f.value(&f.complete(&f.start_future(&future))),
        Value::I32(7)
    );
    assert!(
        f.call("after_spawn_trap", &[capability, f.value(&events)])
            .is_err()
    );
    let admitted = runtime.ready_tasks(&scope).unwrap();
    assert_eq!(
        admitted.len(),
        1,
        "admission survives subsequent handler failure"
    );
    assert_eq!(drive(&f, &scope, admitted[0]), TaskDriveResult::Complete);
    assert_eq!(
        f.value(
            &runtime
                .take_task_report(&scope, admitted[0])
                .unwrap()
                .unwrap()
                .outcome
                .unwrap()
        ),
        Value::I32(9)
    );
    scope.close();
    runtime.drain_cancelled_tasks().unwrap();
    let closed = f.invoke("object", &[capability, f.value(&events)]);
    assert_eq!(
        f.value(&f.invoke("status", &[f.value(&closed)])),
        Value::I32(1)
    );
    let failed_scope = f
        .runtime
        .create_task_scope(
            &f.module,
            &Default::default(),
            Arc::new(|_| Err(DispatchUnavailable)),
        )
        .unwrap();
    let failed = f.invoke(
        "object",
        &[f.value(failed_scope.capability()), f.value(&events)],
    );
    assert_eq!(
        f.value(&f.invoke("status", &[f.value(&failed)])),
        Value::I32(3)
    );
    assert!(runtime.ready_tasks(&failed_scope).unwrap().is_empty());
}
