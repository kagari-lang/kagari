//! Scope ownership, durable readiness and bounded terminal reporting.
use super::{Fixture, slice};
use crate::engine::EngineConfig;
use kagari_runtime::{
    error::{RuntimeErrorKind, fault::EngineFault},
    task::{
        CancellationCause, SpawnError,
        control::{DispatchUnavailable, ReadyNotice, TaskScopeOwner},
        drive::TaskDriveResult,
    },
    value::carriers::RootedValue,
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
    let mut f = Fixture::configured_source(
        include_str!("../../../../../tests/fixtures/embedding/async/task-scope.kgr"),
        config,
    );

    let notices = Arc::new(Mutex::new(Vec::<ReadyNotice>::new()));
    let seen = notices.clone();
    let fail = Arc::new(AtomicBool::new(false));
    let failed = fail.clone();
    let scope = f
        .runtime
        .create_dispatched_task_scope(
            &f.runtime.select_program(&f.module).unwrap(),
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
    let spawn = |scope: &TaskScopeOwner, factory: &RootedValue, f: &mut Fixture| {
        f.runtime
            .spawn_task(scope.capability(), factory)
            .unwrap()
            .unwrap()
    };
    let until_boundary = |scope: &TaskScopeOwner, id, f: &mut Fixture| {
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
        panic!("bounded task drive");
    };
    let items = f.invoke("items", &[]);
    let factory = f.invoke("work", slices::from_ref(&items));
    let task = spawn(&scope, &factory, &mut f);
    let id = f.runtime.task_id(&task).unwrap();
    drop(factory);
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    assert_eq!(f.runtime.ready_tasks(&scope).unwrap(), vec![id]);
    assert_eq!(until_boundary(&scope, id, &mut f), TaskDriveResult::Waiting);
    assert!(
        f.runtime.ready_tasks(&scope).unwrap().is_empty(),
        "initial readiness was consumed"
    );
    let _ = f.invoke("replace", slices::from_ref(&items));
    let simple = f.invoke("simple", &[]);
    let other = spawn(&scope, &simple, &mut f);
    let other_id = f.runtime.task_id(&other).unwrap();
    // Dropping the Host handle cannot retire the runtime-owned queued Task.
    drop(other);
    assert_eq!(
        until_boundary(&scope, other_id, &mut f),
        TaskDriveResult::Complete
    );
    assert!(
        matches!(
            f.runtime.spawn_task(scope.capability(), &simple).unwrap(),
            Err(SpawnError::CapacityExceeded)
        ),
        "unclaimed terminal reports reserve capacity"
    );
    let report = f
        .runtime
        .take_task_report(&scope, other_id)
        .unwrap()
        .unwrap();
    assert_eq!(report.outcome.unwrap().as_i32().unwrap(), 7);
    assert_eq!(
        f.runtime.drive_task(&scope, other_id, slice()).unwrap(),
        TaskDriveResult::Stale
    );
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(11));
    assert!(f.runtime.ready_tasks(&scope).unwrap().contains(&id));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "notification alone cannot run script"
    );
    assert_eq!(until_boundary(&scope, id, &mut f), TaskDriveResult::Waiting);
    assert_eq!(*f.inputs.lock().unwrap(), vec![10, 40]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(41));
    assert_eq!(
        until_boundary(&scope, id, &mut f),
        TaskDriveResult::Complete
    );
    let _ = f.invoke("push", slices::from_ref(&items));
    f.runtime.collect_garbage().unwrap();
    let report = f.runtime.take_task_report(&scope, id).unwrap().unwrap();
    assert_eq!(report.task, id);
    assert_eq!(report.scope, scope.id());
    let result = report.outcome.unwrap();
    f.runtime.collect_garbage().unwrap();
    assert_eq!(
        f.invoke("count", slices::from_ref(&result))
            .as_u64()
            .unwrap(),
        2
    );
    f.runtime.cancel_task(&task).unwrap();
    assert!(f.runtime.take_task_report(&scope, id).unwrap().is_none());
    drop(result);
    f.runtime.collect_garbage().unwrap();
    let cached_future = f.invoke("cached", slices::from_ref(&task));
    let cached = f.run_future(&cached_future);
    drop(cached_future);
    assert_eq!(
        f.invoke("count", slices::from_ref(&cached))
            .as_u64()
            .unwrap(),
        2,
        "Task owns the cached GC edge after report consumption"
    );
    drop(task);
    f.runtime.collect_garbage().unwrap();
    assert_eq!(
        f.invoke("count", slices::from_ref(&cached))
            .as_u64()
            .unwrap(),
        2
    );
    f.runtime.collect_garbage().unwrap();
    let retained_cache = f.runtime.gc_stats();
    drop(cached);
    f.runtime.collect_garbage().unwrap();
    let released_cache = f.runtime.gc_stats();
    assert_eq!(
        released_cache.root_registrations,
        retained_cache.root_registrations - 1
    );
    assert!(
        released_cache.objects < retained_cache.objects,
        "retired task registry does not retain the result after its last Host root"
    );

    let trapped_factory = f.invoke("trapped", &[]);
    let trapped = spawn(&scope, &trapped_factory, &mut f);
    drop(trapped_factory);
    let trapped_id = f.runtime.task_id(&trapped).unwrap();
    assert_eq!(
        until_boundary(&scope, trapped_id, &mut f),
        TaskDriveResult::Complete
    );
    let failure = f
        .runtime
        .take_task_report(&scope, trapped_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(failure.error.kind(), RuntimeErrorKind::ScriptTrap);
    assert_eq!(failure.source_task, trapped_id);
    assert!(failure.error.trace().is_some());

    let factory = f.invoke("work", slices::from_ref(&items));
    let cancelled = spawn(&scope, &factory, &mut f);
    let cancelled_id = f.runtime.task_id(&cancelled).unwrap();
    assert_eq!(
        until_boundary(&scope, cancelled_id, &mut f),
        TaskDriveResult::Waiting
    );
    fail.store(true, Ordering::SeqCst);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(90));
    assert!(matches!(
        f.runtime.spawn_task(scope.capability(), &simple).unwrap(),
        Err(SpawnError::DispatchUnavailable)
    ));
    assert_eq!(f.runtime.drain_cancelled_tasks().unwrap(), 1);
    let failure = f
        .runtime
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
    let _ = f.invoke("push", slices::from_ref(&items));
    scope.replace_dispatcher(Arc::new(|_| Ok(()))).unwrap();
    let queued = spawn(&scope, &simple, &mut f);
    let queued_id = f.runtime.task_id(&queued).unwrap();
    let running = spawn(&scope, &factory, &mut f);
    let running_id = f.runtime.task_id(&running).unwrap();
    assert_eq!(
        until_boundary(&scope, running_id, &mut f),
        TaskDriveResult::Waiting
    );
    drop(running);
    let cancels_before = f.cancels.load(Ordering::SeqCst);
    scope.close();
    assert!(
        !scope.is_closed(),
        "close request precedes cleanup acknowledgement"
    );
    assert_eq!(f.runtime.drain_cancelled_tasks().unwrap(), 2);
    assert_eq!(f.cancels.load(Ordering::SeqCst), cancels_before + 1);
    let _ = f.invoke("push", slices::from_ref(&items));
    assert!(scope.is_closed());
    let failure = f
        .runtime
        .take_task_report(&scope, queued_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(failure.cancellation, Some(CancellationCause::ScopeClose));
    assert!(matches!(
        f.runtime.spawn_task(scope.capability(), &simple).unwrap(),
        Err(SpawnError::ScopeClosed)
    ));

    let rejected = f
        .runtime
        .create_dispatched_task_scope(
            &f.runtime.select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| Err(DispatchUnavailable)),
        )
        .unwrap();
    assert!(
        f.runtime
            .create_dispatched_task_scope(
                &f.runtime.select_program(&f.module).unwrap(),
                Default::default(),
                Arc::new(|_| Ok(()))
            )
            .is_err()
    );
    assert!(
        f.runtime
            .drive_task(&rejected, running_id, slice())
            .is_err(),
        "scope ownership is required"
    );
    let _ = f
        .runtime
        .take_task_report(&scope, running_id)
        .unwrap()
        .unwrap();
    assert!(matches!(
        f.runtime
            .spawn_task(rejected.capability(), &simple)
            .unwrap(),
        Err(SpawnError::DispatchUnavailable)
    ));
    assert!(
        f.runtime.ready_tasks(&rejected).unwrap().is_empty(),
        "failed publication rolls admission back"
    );
    let dispatcher_lifetime = Arc::new(());
    let dispatcher_released = Arc::downgrade(&dispatcher_lifetime);
    rejected
        .replace_dispatcher(Arc::new(move |_| {
            let _retained = &dispatcher_lifetime;
            Ok(())
        }))
        .unwrap();
    let retained_capability = rejected.capability().clone();
    let dropped = spawn(&rejected, &simple, &mut f);
    drop(rejected);
    assert_eq!(f.runtime.drain_cancelled_tasks().unwrap(), 1);
    assert!(
        dispatcher_released.upgrade().is_none(),
        "a retained script capability must not retain the retired Host dispatcher"
    );
    assert!(matches!(
        f.runtime.spawn_task(&retained_capability, &simple).unwrap(),
        Err(SpawnError::ScopeClosed)
    ));
    f.runtime.cancel_task(&dropped).unwrap();
    let replacement = f
        .runtime
        .create_dispatched_task_scope(
            &f.runtime.select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| Ok(())),
        )
        .unwrap();
    let admitted = spawn(&replacement, &simple, &mut f);
    let admitted_id = f.runtime.task_id(&admitted).unwrap();
    assert_ne!(admitted_id, queued_id);
    assert_eq!(
        f.runtime
            .drive_task(&replacement, queued_id, slice())
            .unwrap(),
        TaskDriveResult::Stale
    );
    f.runtime.cancel_task(&admitted).unwrap();
    assert_eq!(
        until_boundary(&replacement, admitted_id, &mut f),
        TaskDriveResult::Complete
    );
    let failure = f
        .runtime
        .take_task_report(&replacement, admitted_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(failure.cancellation, Some(CancellationCause::Explicit));
    let unfinished = spawn(&replacement, &factory, &mut f);
    let unfinished_id = f.runtime.task_id(&unfinished).unwrap();
    assert_eq!(
        until_boundary(&replacement, unfinished_id, &mut f),
        TaskDriveResult::Waiting
    );
    let queued = spawn(&replacement, &simple, &mut f);
    let queued_id = f.runtime.task_id(&queued).unwrap();
    assert_eq!(
        until_boundary(&replacement, queued_id, &mut f),
        TaskDriveResult::Complete
    );
    let _ = f
        .runtime
        .quarantine_execution_invariant(EngineFault::TaskContractFault);
    assert_eq!(f.runtime.drain_cancelled_tasks().unwrap(), 1);
    let failure = f
        .runtime
        .take_task_report(&replacement, unfinished_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap_err();
    assert_eq!(failure.error.kind(), RuntimeErrorKind::EngineFault);
    assert_eq!(failure.cancellation, None);
    let success = f
        .runtime
        .take_task_report(&replacement, queued_id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap();
    assert_eq!(
        success.as_i32().unwrap(),
        7,
        "an unrelated quarantine preserves already completed reports"
    );
    assert!(replacement.is_closed());
}

#[test]
fn scoped_task_shutdown_contract() {
    let mut f = Fixture::source(
        r#"
use test::async_sdk::request;
pub fn work() -> fn()->Future<i32> { async || request(10).await }
"#,
    );
    let scope = f
        .runtime
        .create_dispatched_task_scope(
            &f.runtime.select_program(&f.module).unwrap(),
            Default::default(),
            Arc::new(|_| panic!("dispatcher fault")),
        )
        .unwrap();
    let factory = f.invoke("work", &[]);
    assert!(matches!(
        f.runtime.spawn_task(scope.capability(), &factory).unwrap(),
        Err(SpawnError::DispatchUnavailable)
    ));
    scope.replace_dispatcher(Arc::new(|_| Ok(()))).unwrap();
    let task = f
        .runtime
        .spawn_task(scope.capability(), &factory)
        .unwrap()
        .unwrap();
    let id = f.runtime.task_id(&task).unwrap();
    assert_eq!(
        f.runtime.drive_task(&scope, id, slice()).unwrap(),
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
        f.runtime.drive_task(&scope, id, slice()).unwrap(),
        TaskDriveResult::Complete,
        "old dispatcher failure cannot poison a replacement"
    );
    let report = f.runtime.take_task_report(&scope, id).unwrap().unwrap();
    assert_eq!(report.outcome.unwrap().as_i32().unwrap(), 8);
    let task = f
        .runtime
        .spawn_task(scope.capability(), &factory)
        .unwrap()
        .unwrap();
    let id = f.runtime.task_id(&task).unwrap();
    assert_eq!(
        f.runtime.drive_task(&scope, id, slice()).unwrap(),
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
