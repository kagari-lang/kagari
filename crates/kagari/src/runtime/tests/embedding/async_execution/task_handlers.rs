//! Ordinary registered task methods across the synchronous-handler boundary.
use super::Fixture;
use crate::engine::EngineConfig;
use kagari_runtime::{
    task::{
        CancellationCause, TaskId,
        control::{DispatchUnavailable, TaskScopeOwner},
        drive::TaskDriveResult,
    },
    value::carriers::RootedValue,
};
use std::{
    num::NonZeroUsize,
    slice,
    sync::{Arc, atomic::Ordering},
};

const SOURCE: &str =
    include_str!("../../../../../tests/fixtures/embedding/async/task-handlers.kgr");

fn drive(f: &mut Fixture, scope: &TaskScopeOwner, id: TaskId) -> TaskDriveResult {
    for _ in 0..1000 {
        f.runtime.collect_garbage().unwrap();
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

fn task(f: &mut Fixture, result: &RootedValue) -> (TaskId, RootedValue) {
    let handle = f.invoke("unwrap_i32", slice::from_ref(result));
    (f.runtime.task_id(&handle).unwrap(), handle)
}

#[test]
fn scoped_task_handler_contract() {
    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_tasks = NonZeroUsize::new(2).unwrap();
    let mut f = Fixture::configured_source(SOURCE, config);

    let scope = f
        .runtime
        .create_dispatched_task_scope(
            &f.runtime.select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| Ok(())),
        )
        .unwrap();
    let capability = (scope.capability()).clone();
    let events = f.invoke("events", &[]);
    let items = f.invoke("items", &[]);
    let result = f.invoke(
        "handler",
        &[capability.clone(), items.clone(), events.clone()],
    );
    let (id, handle) = task(&mut f, &result);
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.invoke("len", slice::from_ref(&events)).as_u64().unwrap(),
        1
    );
    assert_eq!(
        f.invoke("at", &[events.clone(), RootedValue::from(0_u64)])
            .as_i32()
            .unwrap(),
        0
    );
    assert_eq!(drive(&mut f, &scope, id), TaskDriveResult::Waiting);
    assert_eq!(f.inputs.lock().unwrap().as_slice(), &[10]);
    assert_eq!(
        f.invoke("len", slice::from_ref(&events)).as_u64().unwrap(),
        2
    );
    let _ = f.invoke("replace", slice::from_ref(&items));
    // A separate synchronous handler captures the running Task and submits a waiter.
    let observer = f.invoke("observer", &[capability.clone(), handle.clone()]);
    let (waiter, waiter_handle) = task(&mut f, &observer);
    assert_eq!(drive(&mut f, &scope, waiter), TaskDriveResult::Waiting);
    let excess = f.invoke("object", &[capability.clone(), events.clone()]);
    assert_eq!(
        f.invoke("status", slice::from_ref(&excess))
            .as_i32()
            .unwrap(),
        2
    );
    assert_eq!(
        f.invoke("len", slice::from_ref(&events)).as_u64().unwrap(),
        2
    );
    let _ = f.invoke("stop_i32", slice::from_ref(&waiter_handle));
    assert_eq!(f.runtime.drain_cancelled_tasks().unwrap(), 1);
    assert_eq!(
        f.runtime
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
    assert_eq!(drive(&mut f, &scope, id), TaskDriveResult::Waiting);
    assert_eq!(f.inputs.lock().unwrap().as_slice(), &[10, 40]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(6));
    assert_eq!(drive(&mut f, &scope, id), TaskDriveResult::Complete);
    let report = f.runtime.take_task_report(&scope, id).unwrap().unwrap();
    assert_eq!(report.outcome.unwrap().as_i32().unwrap(), 11);
    let _ = f.invoke("append", slice::from_ref(&items));
    let object = f.invoke("object", &[capability.clone(), events.clone()]);
    let (object_id, _) = task(&mut f, &object);
    assert_eq!(
        f.invoke("len", slice::from_ref(&events)).as_u64().unwrap(),
        2
    );
    assert_eq!(drive(&mut f, &scope, object_id), TaskDriveResult::Complete);
    assert_eq!(
        f.runtime
            .take_task_report(&scope, object_id)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap()
            .as_i32()
            .unwrap(),
        7
    );
    assert_eq!(
        f.invoke("len", slice::from_ref(&events)).as_u64().unwrap(),
        3
    );
    let nested = f.invoke("nested", slice::from_ref(&capability));
    let nested_handle = f.invoke("unwrap_nested", slice::from_ref(&nested));
    let nested_id = f.runtime.task_id(&nested_handle).unwrap();
    let starts = f.starts.load(Ordering::SeqCst);
    assert_eq!(drive(&mut f, &scope, nested_id), TaskDriveResult::Complete);
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        starts,
        "Task<Future<T>> is not flattened"
    );
    let future = f
        .runtime
        .take_task_report(&scope, nested_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap();
    assert_eq!(f.run_future(&future).as_i32().unwrap(), 7);
    assert!(
        f.call("after_spawn_trap", &[capability.clone(), events.clone()])
            .is_err()
    );
    let admitted = f.runtime.ready_tasks(&scope).unwrap();
    assert_eq!(
        admitted.len(),
        1,
        "admission survives subsequent handler failure"
    );
    assert_eq!(
        drive(&mut f, &scope, admitted[0]),
        TaskDriveResult::Complete
    );
    assert_eq!(
        f.runtime
            .take_task_report(&scope, admitted[0])
            .unwrap()
            .unwrap()
            .outcome
            .unwrap()
            .as_i32()
            .unwrap(),
        9
    );
    scope.close();
    f.runtime.drain_cancelled_tasks().unwrap();
    let closed = f.invoke("object", &[capability.clone(), events.clone()]);
    assert_eq!(
        f.invoke("status", slice::from_ref(&closed))
            .as_i32()
            .unwrap(),
        1
    );
    let failed_scope = f
        .runtime
        .create_dispatched_task_scope(
            &f.runtime.select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| Err(DispatchUnavailable)),
        )
        .unwrap();
    let failed = f.invoke(
        "object",
        &[(failed_scope.capability()).clone(), events.clone()],
    );
    assert_eq!(
        f.invoke("status", slice::from_ref(&failed))
            .as_i32()
            .unwrap(),
        3
    );
    assert!(f.runtime.ready_tasks(&failed_scope).unwrap().is_empty());
}
