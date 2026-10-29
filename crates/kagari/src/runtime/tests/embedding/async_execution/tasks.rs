//! Scope ownership, durable readiness and bounded terminal reporting.
use super::{Fixture, Value, slice};
use crate::engine::EngineConfig;
use kagari_runtime::{
    error::RuntimeErrorKind,
    task::{
        CancellationCause, SpawnError,
        control::{DispatchUnavailable, ReadyNotice, TaskScopeOwner},
        drive::TaskDriveResult,
    },
};
use std::{
    num::NonZeroUsize,
    slice as slices,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

#[test]
fn scoped_task_contract() {
    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_tasks = NonZeroUsize::new(2).unwrap();
    config.default_runtime.async_limits.max_task_scopes = NonZeroUsize::new(2).unwrap();
    let f = Fixture::configured_source(
        include_str!("../../../../../tests/fixtures/embedding/async/task-scope.kgr"),
        config,
    );
    let runtime = f.runtime.runtime();
    let notices = Arc::new(Mutex::new(Vec::<ReadyNotice>::new()));
    let seen = notices.clone();
    let fail = Arc::new(AtomicBool::new(false));
    let failed = fail.clone();
    let scope = f
        .runtime
        .runtime()
        .create_task_scope(
            &f.runtime.runtime().select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(move |notice| {
                seen.lock().unwrap().push(notice);
                if failed.load(Ordering::SeqCst) {
                    Err(DispatchUnavailable)
                } else {
                    Ok(())
                }
            }),
        )
        .unwrap();
    let spawn = |scope: &TaskScopeOwner, factory| {
        unsafe { runtime.spawn_task_transport(&f.value(scope.capability()), &factory) }
            .unwrap()
            .unwrap()
    };
    let until_boundary = |scope: &TaskScopeOwner, id| {
        for _ in 0..1000 {
            runtime.collect_garbage().unwrap();
            let result = f
                .runtime
                .backend()
                .drive_task(scope, id, NonZeroUsize::new(1).unwrap())
                .unwrap();
            if result != TaskDriveResult::Runnable {
                return result;
            }
        }
        panic!("bounded task drive");
    };
    let items = f.invoke("items", &[]);
    let factory = f.invoke("work", &[f.value(&items)]);
    let task = spawn(&scope, f.value(&factory));
    let id = unsafe { runtime.task_id_transport(&f.value(&task)) }.unwrap();
    drop(factory);
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.ready_tasks(&scope).unwrap(), vec![id]);
    assert_eq!(until_boundary(&scope, id), TaskDriveResult::Waiting);
    assert!(
        runtime.ready_tasks(&scope).unwrap().is_empty(),
        "initial readiness was consumed"
    );
    let _ = f.invoke("replace", &[f.value(&items)]);
    let simple = f.invoke("simple", &[]);
    let other = spawn(&scope, f.value(&simple));
    let other_id = unsafe { runtime.task_id_transport(&f.value(&other)) }.unwrap();
    // An exposed host root is mutable; task ownership must keep its own root.
    let unit = unsafe { runtime.gc().root_value_transport(Value::Unit) }.unwrap();
    other.replace(runtime.gc(), &unit).unwrap();
    drop(unit);
    assert_eq!(until_boundary(&scope, other_id), TaskDriveResult::Complete);
    assert!(
        matches!(
            unsafe {
                runtime.spawn_task_transport(&f.value(scope.capability()), &f.value(&simple))
            }
            .unwrap(),
            Err(SpawnError::CapacityExceeded)
        ),
        "unclaimed terminal reports reserve capacity"
    );
    let report = runtime.take_task_report(&scope, other_id).unwrap().unwrap();
    assert_eq!(f.value(&report.outcome.unwrap()), Value::I32(7));
    assert_eq!(
        f.runtime
            .backend()
            .drive_task(&scope, other_id, slice())
            .unwrap(),
        TaskDriveResult::Stale
    );
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(11));
    assert!(runtime.ready_tasks(&scope).unwrap().contains(&id));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "notification alone cannot run script"
    );
    assert_eq!(until_boundary(&scope, id), TaskDriveResult::Waiting);
    assert_eq!(*f.inputs.lock().unwrap(), vec![10, 40]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(41));
    assert_eq!(until_boundary(&scope, id), TaskDriveResult::Complete);
    let _ = f.invoke("push", &[f.value(&items)]);
    runtime.collect_garbage().unwrap();
    let report = runtime.take_task_report(&scope, id).unwrap().unwrap();
    assert_eq!(report.task, id);
    assert_eq!(report.scope, scope.id());
    let result = report.outcome.unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&result)])),
        Value::U64(2)
    );
    unsafe { runtime.cancel_task_transport(&f.value(&task)) }.unwrap();
    assert!(runtime.take_task_report(&scope, id).unwrap().is_none());
    let cached = f.value(&result);
    drop(result);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        f.value(&f.invoke("count", slices::from_ref(&cached))),
        Value::U64(2),
        "Task owns the cached GC edge after report consumption"
    );
    drop(task);
    runtime.collect_garbage().unwrap();
    assert!(
        unsafe { runtime.gc().root_value_transport(cached) }.is_none(),
        "retired task registry does not retain the result"
    );

    let trapped = spawn(&scope, f.value(&f.invoke("trapped", &[])));
    let trapped_id = unsafe { runtime.task_id_transport(&f.value(&trapped)) }.unwrap();
    assert_eq!(
        until_boundary(&scope, trapped_id),
        TaskDriveResult::Complete
    );
    let failure = runtime
        .take_task_report(&scope, trapped_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(failure.error.kind(), RuntimeErrorKind::ScriptTrap);
    assert_eq!(failure.source_task, trapped_id);
    assert!(failure.error.trace().is_some());

    let factory = f.invoke("work", &[f.value(&items)]);
    let cancelled = spawn(&scope, f.value(&factory));
    let cancelled_id = unsafe { runtime.task_id_transport(&f.value(&cancelled)) }.unwrap();
    assert_eq!(
        until_boundary(&scope, cancelled_id),
        TaskDriveResult::Waiting
    );
    fail.store(true, Ordering::SeqCst);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(90));
    assert!(matches!(
        unsafe { runtime.spawn_task_transport(&f.value(scope.capability()), &f.value(&simple)) }
            .unwrap(),
        Err(SpawnError::DispatchUnavailable)
    ));
    assert_eq!(runtime.drain_cancelled_tasks().unwrap(), 1);
    let failure = runtime
        .take_task_report(&scope, cancelled_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(
        failure.cancellation,
        Some(CancellationCause::DispatchFailure)
    );
    assert_eq!(failure.error.kind(), RuntimeErrorKind::Cancelled);
    let _ = f.invoke("push", &[f.value(&items)]);
    scope.replace_dispatcher(Arc::new(|_| Ok(()))).unwrap();
    let queued = spawn(&scope, f.value(&simple));
    let queued_id = unsafe { runtime.task_id_transport(&f.value(&queued)) }.unwrap();
    let running = spawn(&scope, f.value(&factory));
    let running_id = unsafe { runtime.task_id_transport(&f.value(&running)) }.unwrap();
    assert_eq!(until_boundary(&scope, running_id), TaskDriveResult::Waiting);
    drop(running);
    let cancels_before = f.cancels.load(Ordering::SeqCst);
    scope.close();
    assert!(
        !scope.is_closed(),
        "close request precedes cleanup acknowledgement"
    );
    assert_eq!(runtime.drain_cancelled_tasks().unwrap(), 2);
    assert_eq!(f.cancels.load(Ordering::SeqCst), cancels_before + 1);
    let _ = f.invoke("push", &[f.value(&items)]);
    assert!(scope.is_closed());
    let failure = runtime
        .take_task_report(&scope, queued_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(failure.cancellation, Some(CancellationCause::ScopeClose));
    assert!(matches!(
        unsafe { runtime.spawn_task_transport(&f.value(scope.capability()), &f.value(&simple)) }
            .unwrap(),
        Err(SpawnError::ScopeClosed)
    ));

    let rejected = f
        .runtime
        .runtime()
        .create_task_scope(
            &f.runtime.runtime().select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| Err(DispatchUnavailable)),
        )
        .unwrap();
    assert!(
        f.runtime
            .runtime()
            .create_task_scope(
                &f.runtime.runtime().select_program(&f.module).unwrap(),
                Default::default(),
                Arc::new(|_| Ok(()))
            )
            .is_err()
    );
    assert!(
        f.runtime
            .backend()
            .drive_task(&rejected, running_id, slice())
            .is_err(),
        "scope ownership is required"
    );
    let _ = runtime
        .take_task_report(&scope, running_id)
        .unwrap()
        .unwrap();
    assert!(matches!(
        unsafe { runtime.spawn_task_transport(&f.value(rejected.capability()), &f.value(&simple)) }
            .unwrap(),
        Err(SpawnError::DispatchUnavailable)
    ));
    assert!(
        runtime.ready_tasks(&rejected).unwrap().is_empty(),
        "failed publication rolls admission back"
    );
    rejected.replace_dispatcher(Arc::new(|_| Ok(()))).unwrap();
    let retained_capability = rejected.capability().clone();
    let dropped = spawn(&rejected, f.value(&simple));
    drop(rejected);
    assert_eq!(runtime.drain_cancelled_tasks().unwrap(), 1);
    assert!(matches!(
        unsafe { runtime.spawn_task_transport(&f.value(&retained_capability), &f.value(&simple)) }
            .unwrap(),
        Err(SpawnError::ScopeClosed)
    ));
    unsafe { runtime.cancel_task_transport(&f.value(&dropped)) }.unwrap();
    let replacement = f
        .runtime
        .runtime()
        .create_task_scope(
            &f.runtime.runtime().select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| Ok(())),
        )
        .unwrap();
    let admitted = spawn(&replacement, f.value(&simple));
    let admitted_id = unsafe { runtime.task_id_transport(&f.value(&admitted)) }.unwrap();
    assert_ne!(admitted_id, queued_id);
    assert_eq!(
        f.runtime
            .backend()
            .drive_task(&replacement, queued_id, slice())
            .unwrap(),
        TaskDriveResult::Stale
    );
    unsafe { runtime.cancel_task_transport(&f.value(&admitted)) }.unwrap();
    assert_eq!(
        until_boundary(&replacement, admitted_id),
        TaskDriveResult::Complete
    );
    let failure = runtime
        .take_task_report(&replacement, admitted_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(failure.cancellation, Some(CancellationCause::Explicit));
    let unfinished = spawn(&replacement, f.value(&factory));
    let unfinished_id = unsafe { runtime.task_id_transport(&f.value(&unfinished)) }.unwrap();
    assert_eq!(
        until_boundary(&replacement, unfinished_id),
        TaskDriveResult::Waiting
    );
    let queued = spawn(&replacement, f.value(&simple));
    let queued_id = unsafe { runtime.task_id_transport(&f.value(&queued)) }.unwrap();
    assert_eq!(
        until_boundary(&replacement, queued_id),
        TaskDriveResult::Complete
    );
    let _ = runtime.quarantine_execution_invariant("task contract fault");
    assert_eq!(runtime.drain_cancelled_tasks().unwrap(), 1);
    let failure = runtime
        .take_task_report(&replacement, unfinished_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(failure.error.kind(), RuntimeErrorKind::EngineFault);
    assert_eq!(failure.cancellation, None);
    let success = runtime
        .take_task_report(&replacement, queued_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap();
    assert_eq!(
        f.value(&success),
        Value::I32(7),
        "an unrelated quarantine preserves already completed reports"
    );
    assert!(replacement.is_closed());
}

#[test]
fn scoped_task_shutdown_contract() {
    let f = Fixture::source(
        r#"
use test::async_sdk::request;
pub fn work() -> fn()->Future<i32> { async || request(10).await }
"#,
    );
    let scope = f
        .runtime
        .runtime()
        .create_task_scope(
            &f.runtime.runtime().select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| panic!("dispatcher fault")),
        )
        .unwrap();
    let factory = f.invoke("work", &[]);
    assert!(matches!(
        unsafe {
            f.runtime
                .runtime()
                .spawn_task_transport(&f.value(scope.capability()), &f.value(&factory))
        }
        .unwrap(),
        Err(SpawnError::DispatchUnavailable)
    ));
    scope.replace_dispatcher(Arc::new(|_| Ok(()))).unwrap();
    let task = unsafe {
        f.runtime
            .runtime()
            .spawn_task_transport(&f.value(scope.capability()), &f.value(&factory))
    }
    .unwrap()
    .unwrap();
    let id = unsafe { f.runtime.runtime().task_id_transport(&f.value(&task)) }.unwrap();
    assert_eq!(
        f.runtime.backend().drive_task(&scope, id, slice()).unwrap(),
        TaskDriveResult::Waiting
    );
    let (entered, wait_entered) = mpsc::sync_channel(1);
    let (release, wait_release) = mpsc::sync_channel(1);
    let wait_release = Mutex::new(wait_release);
    scope
        .replace_dispatcher(Arc::new(move |notice: ReadyNotice| {
            if notice.task.is_some() {
                entered.send(()).unwrap();
                wait_release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                Err(DispatchUnavailable)
            } else {
                Ok(())
            }
        }))
        .unwrap();
    let completion = f.sent.lock().unwrap().pop().unwrap();
    let producer = thread::spawn(move || completion.complete(Ok(8)));
    wait_entered.recv_timeout(Duration::from_secs(5)).unwrap();
    scope.replace_dispatcher(Arc::new(|_| Ok(()))).unwrap();
    release.send(()).unwrap();
    producer.join().unwrap();
    assert_eq!(
        f.runtime.backend().drive_task(&scope, id, slice()).unwrap(),
        TaskDriveResult::Complete,
        "old dispatcher failure cannot poison a replacement"
    );
    let report = f
        .runtime
        .runtime()
        .take_task_report(&scope, id)
        .unwrap()
        .unwrap();
    assert_eq!(f.value(&report.outcome.unwrap()), Value::I32(8));
    let task = unsafe {
        f.runtime
            .runtime()
            .spawn_task_transport(&f.value(scope.capability()), &f.value(&factory))
    }
    .unwrap()
    .unwrap();
    let id = unsafe { f.runtime.runtime().task_id_transport(&f.value(&task)) }.unwrap();
    assert_eq!(
        f.runtime.backend().drive_task(&scope, id, slice()).unwrap(),
        TaskDriveResult::Waiting
    );
    let cancels = f.cancels.clone();
    assert!(!scope.is_closed());
    drop(f);
    assert_eq!(cancels.load(Ordering::SeqCst), 1);
    assert!(
        scope.is_closed(),
        "runtime destruction drains tasks before acknowledging scope close"
    );
}
