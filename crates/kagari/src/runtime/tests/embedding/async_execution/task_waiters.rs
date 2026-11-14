//! Shared waits, directional cancellation, cached outputs and dependency cycles.

/* stdlib test support paused (NC01-NC06).
struct FailingWake; */

/* stdlib test support paused (NC01-NC06).
impl Wake for FailingWake {
    fn wake(self: Arc<Self>) {
        panic!("host waiter wake failure");
    }
} */

/* stdlib test support paused (NC01-NC06).
const SOURCE: &str = include_str!("../../../../../tests/fixtures/embedding/async/task-waiters.kgr"); */

/* stdlib test support paused (NC01-NC06).
struct Tasks {
    fixture: Fixture,
    scope: TaskScopeOwner,
} */

/* stdlib test support paused (NC01-NC06).
impl Tasks {
    fn new() -> Self {
        let mut config = EngineConfig::default();
        config.default_runtime.async_limits.max_task_waiters = NonZeroUsize::new(2).unwrap();
        let mut fixture = Fixture::configured_source(SOURCE, config);
        let selected = fixture.runtime.select_program(&fixture.module).unwrap();
        let scope = fixture
            .runtime
            .create_dispatched_task_scope(&selected, Default::default(), Arc::new(|_| Ok(())))
            .unwrap();
        Self { fixture, scope }
    }

    fn spawn(&mut self, entry: &str, args: &[RootedValue]) -> (TaskId, RootedValue) {
        let factory = self.fixture.invoke(entry, args);
        let task = self
            .fixture
            .runtime
            .spawn_task(&(self.scope.capability()).clone(), &factory)
            .unwrap()
            .unwrap();
        (self.fixture.runtime.task_id(&task).unwrap(), task)
    }

    fn drive(&mut self, id: TaskId) -> TaskDriveResult {
        for _ in 0..1000 {
            self.fixture.runtime.collect_garbage().unwrap();
            let result = self
                .fixture
                .runtime
                .drive_task(&self.scope, id, NonZeroUsize::new(1).unwrap())
                .unwrap();
            if result != TaskDriveResult::Runnable {
                return result;
            }
        }
        panic!("bounded Task continuation");
    }

    fn report(&mut self, id: TaskId) -> TaskReport {
        self.fixture
            .runtime
            .take_task_report(&self.scope, id)
            .unwrap()
            .unwrap()
    }
} */

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn scoped_task_shared_wait_contract() {
    let mut tasks = Tasks::new();

    let (producer, handle) = tasks.spawn("producer", &[]);
    assert_eq!(tasks.drive(producer), TaskDriveResult::Waiting);
    let (left, left_handle) = tasks.spawn("observe", &[handle.clone(), RootedValue::from(1_i32)]);
    let collection = tasks.fixture.invoke("tasks", &[]);
    for _ in 0..2 {
        let _ = tasks
            .fixture
            .invoke("add", &[collection.clone(), handle.clone()]);
    }
    let (right, _) = tasks.spawn("looping", slices::from_ref(&collection));
    assert_eq!(tasks.drive(left), TaskDriveResult::Waiting);
    assert_eq!(tasks.drive(right), TaskDriveResult::Waiting);
    // Writes commit while the waiter is suspended; its next cursor read traps.
    drop(
        tasks
            .fixture
            .invoke("add", &[collection.clone(), handle.clone()]),
    );
    let (excess, _) = tasks.spawn("observe", &[handle.clone(), RootedValue::from(2_i32)]);
    assert_eq!(tasks.drive(excess), TaskDriveResult::Complete);
    assert_eq!(
        tasks.report(excess).outcome.unwrap_err().error.kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    tasks.fixture.runtime.cancel_task(&left_handle).unwrap();
    assert_eq!(tasks.fixture.runtime.drain_cancelled_tasks().unwrap(), 1);
    assert_eq!(
        tasks.report(left).outcome.unwrap_err().cancellation,
        Some(CancellationCause::Explicit)
    );
    assert_eq!(
        tasks.fixture.cancels.load(Ordering::SeqCst),
        0,
        "waiter cancellation leaves target IO running"
    );
    let (replacement, _) = tasks.spawn("observe", &[handle.clone(), RootedValue::from(2_i32)]);
    assert_eq!(
        tasks.drive(replacement),
        TaskDriveResult::Waiting,
        "cancelled waiter releases capacity"
    );
    tasks
        .fixture
        .sent
        .lock()
        .unwrap()
        .pop()
        .unwrap()
        .complete(Ok(7));
    assert_eq!(
        tasks.fixture.runtime.ready_tasks(&tasks.scope).unwrap(),
        vec![producer]
    );
    assert_eq!(tasks.drive(producer), TaskDriveResult::Complete);
    let ready = tasks.fixture.runtime.ready_tasks(&tasks.scope).unwrap();
    assert!(ready.contains(&right) && ready.contains(&replacement));
    assert_eq!(tasks.report(producer).outcome.unwrap().as_i32().unwrap(), 7);
    let (reused, _) = tasks.spawn("immediate", &[]);
    assert_ne!(reused, producer);
    assert_eq!(tasks.drive(reused), TaskDriveResult::Complete);
    assert_eq!(tasks.report(reused).outcome.unwrap().as_i32().unwrap(), 99);
    assert_eq!(tasks.drive(right), TaskDriveResult::Complete);
    assert_eq!(
        tasks.report(right).outcome.unwrap_err().error.kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(tasks.drive(replacement), TaskDriveResult::Complete);
    assert_eq!(
        tasks.report(replacement).outcome.unwrap().as_i32().unwrap(),
        9
    );
    // A fresh cursor sees all three committed entries and reuses the cached output.
    let (fresh, _) = tasks.spawn("looping", slices::from_ref(&collection));
    assert_eq!(tasks.drive(fresh), TaskDriveResult::Complete);
    assert_eq!(tasks.report(fresh).outcome.unwrap().as_i32().unwrap(), 21);
    let ordinary = tasks.fixture.invoke("direct", slices::from_ref(&handle));
    assert_eq!(tasks.fixture.run_future(&ordinary).as_i32().unwrap(), 7);
    assert_eq!(
        tasks.fixture.starts.load(Ordering::SeqCst),
        1,
        "shared awaits never restart the producer"
    );
    let (business, business_handle) = tasks.spawn("business", &[]);
    let (consumer, _) = tasks.spawn("propagate", slices::from_ref(&business_handle));
    assert_eq!(tasks.drive(consumer), TaskDriveResult::Waiting);
    assert_eq!(tasks.drive(business), TaskDriveResult::Complete);
    assert_eq!(tasks.drive(consumer), TaskDriveResult::Complete);
    let result = tasks.report(consumer).outcome.unwrap();
    assert_eq!(
        tasks
            .fixture
            .invoke("code", slices::from_ref(&result))
            .as_i32()
            .unwrap(),
        -4
    );
    assert!(tasks.report(business).outcome.is_ok());
    let inferred = tasks.fixture.invoke("infer_future", &[]);
    let _ = tasks.fixture.run_future(&inferred);
    let (nested, nested_handle) = tasks.spawn("nested", &[]);
    let (observer, _) = tasks.spawn("nested_observer", slices::from_ref(&nested_handle));
    assert_eq!(tasks.drive(observer), TaskDriveResult::Waiting);
    assert_eq!(tasks.drive(nested), TaskDriveResult::Complete);
    assert_eq!(tasks.drive(observer), TaskDriveResult::Complete);
    let inner = tasks.report(nested).outcome.unwrap();
    let alias = tasks.report(observer).outcome.unwrap();
    assert_eq!(
        tasks.fixture.starts.load(Ordering::SeqCst),
        1,
        "Task await does not flatten Future-valued output"
    );
    let owned = tasks.fixture.start_future(&inner);
    assert!(matches!(
        tasks.fixture.runtime.drive(&owned, slice()).unwrap(),
        DriveResult::Waiting
    ));
    tasks
        .fixture
        .sent
        .lock()
        .unwrap()
        .pop()
        .unwrap()
        .complete(Ok(55));
    assert_eq!(tasks.fixture.complete(&owned).as_i32().unwrap(), 55);
    let repeated = tasks.fixture.start_future(&alias);
    assert!(
        matches!(
            tasks.fixture.runtime.drive(&repeated, slice()).unwrap(),
            DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::ScriptTrap)
        ),
        "shared Task output does not duplicate a Future's single-drive state"
    );
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn scoped_task_dependency_failure_contract() {
    let mut tasks = Tasks::new();

    let (producer, handle) = tasks.spawn("producer", &[]);
    assert_eq!(tasks.drive(producer), TaskDriveResult::Waiting);
    let ordinary = tasks.fixture.invoke("direct", slices::from_ref(&handle));
    let owned = tasks.fixture.start_future(&ordinary);
    assert!(matches!(
        tasks.fixture.runtime.drive(&owned, slice()).unwrap(),
        DriveResult::Waiting
    ));
    owned.cancel();
    assert!(matches!(
        tasks.fixture.runtime.drive(&owned, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::Cancelled)
    ));
    assert_eq!(tasks.fixture.cancels.load(Ordering::SeqCst), 0);
    let other_scope = tasks
        .fixture
        .runtime
        .create_dispatched_task_scope(
            &tasks
                .fixture
                .runtime
                .select_program(&tasks.fixture.module)
                .unwrap(),
            Default::default(),
            Arc::new(|_| Ok(())),
        )
        .unwrap();
    let factory = tasks
        .fixture
        .invoke("observe", &[handle.clone(), RootedValue::from(1_i32)]);
    let other = tasks
        .fixture
        .runtime
        .spawn_task(&(other_scope.capability()).clone(), &factory)
        .unwrap()
        .unwrap();
    let other_id = tasks.fixture.runtime.task_id(&other).unwrap();
    assert_eq!(
        tasks
            .fixture
            .runtime
            .drive_task(&other_scope, other_id, slice())
            .unwrap(),
        TaskDriveResult::Waiting
    );
    other_scope.close();
    assert_eq!(tasks.fixture.runtime.drain_cancelled_tasks().unwrap(), 1);
    assert_eq!(
        tasks
            .fixture
            .runtime
            .take_task_report(&other_scope, other_id)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap_err()
            .cancellation,
        Some(CancellationCause::ScopeClose)
    );
    assert_eq!(
        tasks.fixture.cancels.load(Ordering::SeqCst),
        0,
        "closing a waiting scope leaves another scope's target running"
    );
    let (middle, middle_handle) =
        tasks.spawn("observe", &[handle.clone(), RootedValue::from(1_i32)]);
    let (last, _) = tasks.spawn(
        "observe",
        &[middle_handle.clone(), RootedValue::from(2_i32)],
    );
    assert_eq!(tasks.drive(middle), TaskDriveResult::Waiting);
    assert_eq!(tasks.drive(last), TaskDriveResult::Waiting);
    tasks.fixture.runtime.cancel_task(&handle).unwrap();
    assert_eq!(tasks.fixture.runtime.drain_cancelled_tasks().unwrap(), 1);
    assert_eq!(tasks.fixture.cancels.load(Ordering::SeqCst), 1);
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
    let ordinary = tasks.fixture.invoke("direct", slices::from_ref(&handle));
    let owned = tasks.fixture.start_future(&ordinary);
    let DriveResult::Complete(Err(error)) = tasks.fixture.runtime.drive(&owned, slice()).unwrap()
    else {
        panic!("cached Task failure");
    };

    assert_eq!(error.task_origin().unwrap().task, producer);
    for size in [1, 2, 3] {
        let collection = tasks.fixture.invoke("tasks", &[]);
        let mut ids = vec![];
        for index in 0..size {
            let (id, task) = tasks.spawn(
                "indexed",
                &[
                    collection.clone(),
                    RootedValue::from(((index + 1) % size) as u64),
                ],
            );
            ids.push(id);
            let _ = tasks
                .fixture
                .invoke("add", &[collection.clone(), task.clone()]);
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
            assert_eq!(failure.error.to_string(), "Task wait cycle");
            assert_eq!(failure.source_task, origin);
        }
    }
    let (target, handle) = tasks.spawn("immediate", &[]);
    let ordinary = tasks.fixture.invoke("direct", slices::from_ref(&handle));
    let owned = tasks.fixture.start_future(&ordinary);
    assert!(matches!(
        tasks.fixture.runtime.drive(&owned, slice()).unwrap(),
        DriveResult::Waiting
    ));
    owned.set_waker(&Waker::from(Arc::new(FailingWake)));
    assert!(matches!(
        tasks.fixture.runtime

            .drive_task(&tasks.scope, target, slice())
            ,
        Err(ref error) if error.kind() == RuntimeErrorKind::EngineFault
    ));
    assert!(tasks.fixture.runtime.is_quarantined());
    assert_eq!(
        tasks.report(target).outcome.unwrap().as_i32().unwrap(),
        99,
        "wake failure cannot overwrite committed terminal success"
    );
    assert_eq!(tasks.fixture.runtime.drain_retired_executions().unwrap(), 1);
    tasks.fixture.runtime.drain_cancelled_tasks().unwrap();
}
*/
