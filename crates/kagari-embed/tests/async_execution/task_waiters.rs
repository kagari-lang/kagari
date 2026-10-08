//! Shared waits, directional cancellation, cached outputs and dependency cycles.
use super::{Fixture, Value, slice};
use kagari_embed::{
    engine::EngineConfig,
    error::{EmbeddingError, RuntimeFailureKind},
    runtime::owned::DriveResult,
};
use kagari_runtime::{
    error::RuntimeErrorKind,
    gc::roots::RootedValue,
    task::{
        CancellationCause, TaskId, TaskReport, control::TaskScopeOwner, drive::TaskDriveResult,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{Arc, atomic::Ordering},
    task::{Wake, Waker},
};

struct FailingWake;

impl Wake for FailingWake {
    fn wake(self: Arc<Self>) {
        panic!("host waiter wake failure");
    }
}

const SOURCE: &str = r#"
use test::async_sdk::request;
fn producer() -> fn()->Future<i32> { async || request(10).await }
fn immediate() -> fn()->Future<i32> { async || 99 }
fn observe(task:Task<i32>, extra:i32) -> fn()->Future<i32> { async || { val result:i32=task.await; result + extra } }
async fn direct(task:Task<i32>) -> i32 { task.await }
fn tasks() -> Vec<Task<i32>> { [] }
fn add(tasks:Vec<Task<i32>>, task:Task<i32>) { tasks.push(task); }
fn looping(tasks:Vec<Task<i32>>) -> fn()->Future<i32> {
    async || { var total=0; for task in tasks { total += task.await; } total }
}
fn indexed(tasks:Vec<Task<i32>>, index:usize) -> fn()->Future<i32> { async || tasks[index].await }
fn business() -> fn()->Future<Result<i32,i32>> { async || Err(4) }
fn propagate(task:Task<Result<i32,i32>>) -> fn()->Future<Result<i32,i32>> { async || Ok(task.await? + 1) }
fn code(value:Result<i32,i32>) -> i32 { match value { Ok(x) => x, Err(e) => -e } }
async fn empty<T>() -> Vec<T> { [] }
async fn infer_future() -> Vec<i32> { empty().await }
fn nested() -> fn()->Future<Future<i32>> { async || request(10) }
fn nested_observer(task:Task<Future<i32>>) -> fn()->Future<Future<i32>> { async || task.await }
"#;

struct Tasks {
    fixture: Fixture,
    scope: TaskScopeOwner,
}

impl Tasks {
    fn new() -> Self {
        let mut config = EngineConfig::default();
        config.default_runtime.async_limits.max_task_waiters = NonZeroUsize::new(2).unwrap();
        let fixture = Fixture::configured_source(SOURCE, config);
        let scope = fixture
            .runtime
            .create_task_scope(&fixture.module, &Default::default(), Arc::new(|_| Ok(())))
            .unwrap();
        Self { fixture, scope }
    }

    fn spawn(&self, entry: &str, args: &[Value]) -> (TaskId, RootedValue) {
        let f = &self.fixture;
        let factory = f.invoke(entry, args);
        let task = f
            .runtime
            .runtime()
            .spawn_task(&f.value(self.scope.capability()), &f.value(&factory))
            .unwrap()
            .unwrap();
        (f.runtime.runtime().task_id(&f.value(&task)).unwrap(), task)
    }

    fn drive(&self, id: TaskId) -> TaskDriveResult {
        let f = &self.fixture;
        for _ in 0..1000 {
            f.runtime.runtime().collect_garbage().unwrap();
            let result = f
                .runtime
                .drive_task(&self.scope, id, NonZeroUsize::new(1).unwrap())
                .unwrap();
            if result != TaskDriveResult::Runnable {
                return result;
            }
        }
        panic!("bounded Task continuation");
    }

    fn report(&self, id: TaskId) -> TaskReport {
        self.fixture
            .runtime
            .runtime()
            .take_task_report(&self.scope, id)
            .unwrap()
            .unwrap()
    }
}

#[test]
fn scoped_task_shared_wait_contract() {
    let tasks = Tasks::new();
    let f = &tasks.fixture;
    let runtime = f.runtime.runtime();
    let (producer, handle) = tasks.spawn("producer", &[]);
    assert_eq!(tasks.drive(producer), TaskDriveResult::Waiting);
    let (left, left_handle) = tasks.spawn("observe", &[f.value(&handle), Value::I32(1)]);
    let collection = f.invoke("tasks", &[]);
    for _ in 0..2 {
        let _ = f.invoke("add", &[f.value(&collection), f.value(&handle)]);
    }
    let (right, _) = tasks.spawn("looping", &[f.value(&collection)]);
    assert_eq!(tasks.drive(left), TaskDriveResult::Waiting);
    assert_eq!(tasks.drive(right), TaskDriveResult::Waiting);
    assert!(
        f.call("add", &[f.value(&collection), f.value(&handle)])
            .is_err()
    );
    let (excess, _) = tasks.spawn("observe", &[f.value(&handle), Value::I32(2)]);
    assert_eq!(tasks.drive(excess), TaskDriveResult::Complete);
    assert_eq!(
        tasks.report(excess).outcome.unwrap_err().error.kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    runtime.cancel_task(&f.value(&left_handle)).unwrap();
    assert_eq!(runtime.drain_cancelled_tasks().unwrap(), 1);
    assert_eq!(
        tasks.report(left).outcome.unwrap_err().cancellation,
        Some(CancellationCause::Explicit)
    );
    assert_eq!(
        f.cancels.load(Ordering::SeqCst),
        0,
        "waiter cancellation leaves target IO running"
    );
    let (replacement, _) = tasks.spawn("observe", &[f.value(&handle), Value::I32(2)]);
    assert_eq!(
        tasks.drive(replacement),
        TaskDriveResult::Waiting,
        "cancelled waiter releases capacity"
    );
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(7));
    assert_eq!(runtime.ready_tasks(&tasks.scope).unwrap(), vec![producer]);
    assert_eq!(tasks.drive(producer), TaskDriveResult::Complete);
    let ready = runtime.ready_tasks(&tasks.scope).unwrap();
    assert!(ready.contains(&right) && ready.contains(&replacement));
    assert_eq!(
        f.value(&tasks.report(producer).outcome.unwrap()),
        Value::I32(7)
    );
    let (reused, _) = tasks.spawn("immediate", &[]);
    assert_ne!(reused, producer);
    assert_eq!(tasks.drive(reused), TaskDriveResult::Complete);
    assert_eq!(
        f.value(&tasks.report(reused).outcome.unwrap()),
        Value::I32(99)
    );
    assert_eq!(tasks.drive(right), TaskDriveResult::Complete);
    assert_eq!(
        f.value(&tasks.report(right).outcome.unwrap()),
        Value::I32(14)
    );
    assert_eq!(tasks.drive(replacement), TaskDriveResult::Complete);
    assert_eq!(
        f.value(&tasks.report(replacement).outcome.unwrap()),
        Value::I32(9)
    );
    let _ = f.invoke("add", &[f.value(&collection), f.value(&handle)]);
    let ordinary = f.invoke("direct", &[f.value(&handle)]);
    assert_eq!(
        f.value(&f.complete(&f.start_future(&ordinary))),
        Value::I32(7)
    );
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "shared awaits never restart the producer"
    );
    let (business, business_handle) = tasks.spawn("business", &[]);
    let (consumer, _) = tasks.spawn("propagate", &[f.value(&business_handle)]);
    assert_eq!(tasks.drive(consumer), TaskDriveResult::Waiting);
    assert_eq!(tasks.drive(business), TaskDriveResult::Complete);
    assert_eq!(tasks.drive(consumer), TaskDriveResult::Complete);
    let result = tasks.report(consumer).outcome.unwrap();
    assert_eq!(
        f.value(&f.invoke("code", &[f.value(&result)])),
        Value::I32(-4)
    );
    assert!(tasks.report(business).outcome.is_ok());
    let inferred = f.invoke("infer_future", &[]);
    let _ = f.complete(&f.start_future(&inferred));
    let (nested, nested_handle) = tasks.spawn("nested", &[]);
    let (observer, _) = tasks.spawn("nested_observer", &[f.value(&nested_handle)]);
    assert_eq!(tasks.drive(observer), TaskDriveResult::Waiting);
    assert_eq!(tasks.drive(nested), TaskDriveResult::Complete);
    assert_eq!(tasks.drive(observer), TaskDriveResult::Complete);
    let inner = tasks.report(nested).outcome.unwrap();
    let alias = tasks.report(observer).outcome.unwrap();
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "Task await does not flatten Future-valued output"
    );
    assert_eq!(f.value(&inner), f.value(&alias));
    let owned = f.start_future(&inner);
    assert!(matches!(
        f.runtime.drive(&owned, slice()).unwrap(),
        DriveResult::Waiting
    ));
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(55));
    assert_eq!(f.value(&f.complete(&owned)), Value::I32(55));
    let repeated = f.start_future(&alias);
    assert!(
        matches!(
            f.runtime.drive(&repeated, slice()).unwrap(),
            DriveResult::Complete(Err(EmbeddingError::Runtime {
                kind: RuntimeFailureKind::ScriptTrap,
                ..
            }))
        ),
        "shared Task output does not duplicate a Future's single-drive state"
    );
}

#[test]
fn scoped_task_dependency_failure_contract() {
    let tasks = Tasks::new();
    let f = &tasks.fixture;
    let runtime = f.runtime.runtime();
    let (producer, handle) = tasks.spawn("producer", &[]);
    assert_eq!(tasks.drive(producer), TaskDriveResult::Waiting);
    let ordinary = f.invoke("direct", &[f.value(&handle)]);
    let owned = f.start_future(&ordinary);
    assert!(matches!(
        f.runtime.drive(&owned, slice()).unwrap(),
        DriveResult::Waiting
    ));
    owned.cancel();
    assert!(matches!(
        f.runtime.drive(&owned, slice()).unwrap(),
        DriveResult::Complete(Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::Cancelled,
            ..
        }))
    ));
    assert_eq!(f.cancels.load(Ordering::SeqCst), 0);
    let other_scope = f
        .runtime
        .create_task_scope(&f.module, &Default::default(), Arc::new(|_| Ok(())))
        .unwrap();
    let factory = f.invoke("observe", &[f.value(&handle), Value::I32(1)]);
    let other = runtime
        .spawn_task(&f.value(other_scope.capability()), &f.value(&factory))
        .unwrap()
        .unwrap();
    let other_id = runtime.task_id(&f.value(&other)).unwrap();
    assert_eq!(
        f.runtime
            .drive_task(&other_scope, other_id, slice())
            .unwrap(),
        TaskDriveResult::Waiting
    );
    other_scope.close();
    assert_eq!(runtime.drain_cancelled_tasks().unwrap(), 1);
    assert_eq!(
        runtime
            .take_task_report(&other_scope, other_id)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap_err()
            .cancellation,
        Some(CancellationCause::ScopeClose)
    );
    assert_eq!(
        f.cancels.load(Ordering::SeqCst),
        0,
        "closing a waiting scope leaves another scope's target running"
    );
    let (middle, middle_handle) = tasks.spawn("observe", &[f.value(&handle), Value::I32(1)]);
    let (last, _) = tasks.spawn("observe", &[f.value(&middle_handle), Value::I32(2)]);
    assert_eq!(tasks.drive(middle), TaskDriveResult::Waiting);
    assert_eq!(tasks.drive(last), TaskDriveResult::Waiting);
    runtime.cancel_task(&f.value(&handle)).unwrap();
    assert_eq!(runtime.drain_cancelled_tasks().unwrap(), 1);
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(
        tasks.report(producer).outcome.unwrap_err().cancellation,
        Some(CancellationCause::Explicit)
    );
    for id in [middle, last] {
        assert_eq!(tasks.drive(id), TaskDriveResult::Complete);
        let failure = tasks.report(id).outcome.unwrap_err();
        assert_eq!(failure.source_task, producer);
        assert_eq!(failure.error.kind(), RuntimeErrorKind::Cancelled);
        assert_eq!(failure.cancellation, Some(CancellationCause::Dependency));
        let origin = failure.error.task_origin().unwrap();
        assert_eq!(origin.task, producer);
        assert_eq!(origin.cancellation, Some(CancellationCause::Explicit));
    }
    let ordinary = f.invoke("direct", &[f.value(&handle)]);
    let owned = f.start_future(&ordinary);
    let DriveResult::Complete(Err(error)) = f.runtime.drive(&owned, slice()).unwrap() else {
        panic!("cached Task failure");
    };
    assert_eq!(error.task_origin().unwrap().task, producer);
    for size in [1, 2, 3] {
        let collection = f.invoke("tasks", &[]);
        let mut ids = vec![];
        for index in 0..size {
            let (id, task) = tasks.spawn(
                "indexed",
                &[
                    f.value(&collection),
                    Value::U64(((index + 1) % size) as u64),
                ],
            );
            ids.push(id);
            let _ = f.invoke("add", &[f.value(&collection), f.value(&task)]);
        }
        for id in &ids[..size - 1] {
            assert_eq!(tasks.drive(*id), TaskDriveResult::Waiting);
        }
        let origin = ids[size - 1];
        assert_eq!(tasks.drive(origin), TaskDriveResult::Complete);
        for id in ids.into_iter().rev() {
            assert_eq!(tasks.drive(id), TaskDriveResult::Complete);
            let failure = tasks.report(id).outcome.unwrap_err();
            assert_eq!(failure.error.kind(), RuntimeErrorKind::ScriptTrap);
            assert_eq!(failure.error.message(), "Task wait cycle");
            assert_eq!(failure.source_task, origin);
        }
    }
    let (target, handle) = tasks.spawn("immediate", &[]);
    let ordinary = f.invoke("direct", &[f.value(&handle)]);
    let owned = f.start_future(&ordinary);
    assert!(matches!(
        f.runtime.drive(&owned, slice()).unwrap(),
        DriveResult::Waiting
    ));
    owned.set_waker(&Waker::from(Arc::new(FailingWake)));
    assert!(matches!(
        f.runtime.drive_task(&tasks.scope, target, slice()),
        Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::EngineInvariant,
            ..
        })
    ));
    assert!(runtime.is_quarantined());
    assert_eq!(
        f.value(&tasks.report(target).outcome.unwrap()),
        Value::I32(99),
        "wake failure cannot overwrite committed terminal success"
    );
    assert_eq!(runtime.drain_retired_executions().unwrap(), 1);
    runtime.drain_cancelled_tasks().unwrap();
}
