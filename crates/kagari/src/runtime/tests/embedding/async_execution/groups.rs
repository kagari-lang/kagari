//! Structured pair contracts on the same notification-driven scheduler as Tasks.

/* stdlib test support paused (NC01-NC06).
const SOURCE: &str = include_str!("../../../../../tests/fixtures/embedding/async/groups.kgr"); */

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn groups_waits_are_nonowning_and_cycles_cross_nested_groups() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let scope = f
        .runtime
        .create_task_scope_with_context(&f.module, &Default::default())
        .unwrap();
    let task = f.invoke("launch", &[(scope.capability()).clone()]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 1);
    let future = f.invoke("shared", slice::from_ref(&task));
    let selected_execution = f.start_future(&future);
    let mut joined = f
        .runtime
        .schedule_execution(selected_execution, |_, _| Ok(()))
        .unwrap();
    host.until(&mut f.runtime, || joined.is_finished());
    result(&mut joined).unwrap();
    assert_eq!(
        f.cancels.load(Ordering::SeqCst),
        0,
        "group exit detaches a wait, not its shared Task"
    );
    let mut observer = scheduled("observe", slice::from_ref(&task), &mut f);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(42));
    host.until(&mut f.runtime, || observer.is_finished());
    assert_eq!(result(&mut observer).unwrap(), 42);
    for nested in [false, true] {
        let tasks = f.invoke("tasks", &[]);
        let task = f.invoke(
            "cycle",
            &[
                (scope.capability()).clone(),
                tasks.clone(),
                RootedValue::from(nested),
            ],
        );
        drop(f.invoke("add", &[tasks.clone(), task.clone()]));
        let mut observer = scheduled("observe", slice::from_ref(&task), &mut f);
        host.until(&mut f.runtime, || observer.is_finished());
        let error = result(&mut observer).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert!(error.to_string().contains("cycle"), "{error}");
        assert!(
            f.runtime.take_task_event().is_some(),
            "only the independent Task reports failure"
        );
        assert!(f.runtime.take_task_event().is_none());
    }
    host.settle(&mut f.runtime);
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn groups_pre_admission_cancellation_preserves_inputs_and_releases_capacity() {
    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_groups = NonZeroUsize::new(1).unwrap();
    let mut f = Fixture::configured_source(SOURCE, config);
    let left = f.invoke("input", &[]);
    let right = f.invoke("input", &[]);
    let composition = f.invoke("compose", &[left.clone(), right.clone()]);
    let owner = f.start_future(&composition);
    assert!(matches!(
        f.runtime
            .drive(&owner, NonZeroUsize::new(256).unwrap())
            .unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "parent parks before admission"
    );
    let blocked = {
        let future = f.invoke("cold", &[]);
        f.start_future(&future)
    };
    assert!(matches!(
        f.runtime
            .drive(&blocked, NonZeroUsize::new(256).unwrap())
            .unwrap(),
        DriveResult::Complete(Err(_))
    ));
    drop(owner);
    f.runtime.drain_retired_executions().unwrap();
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut ticket = scheduled("supplied", &[left.clone(), right.clone()], &mut f);
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(
        result(&mut ticket).unwrap(),
        14,
        "cancel before admission claims neither input and releases capacity"
    );
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn groups_cold_dual_io_order_and_result_policy() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let cold = f.invoke("cold", &[]);
    f.runtime.collect_garbage().unwrap();
    host.settle(&mut f.runtime);
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    drop(cold);
    let mut ticket = scheduled("pair", &[], &mut f);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 2);
    host.settle(&mut f.runtime);
    assert_eq!(*f.inputs.lock().unwrap(), vec![10, 20]);
    let right = f.sent.lock().unwrap().pop().unwrap();
    assert_eq!(right.complete(Ok(2)), CompletionStatus::Accepted);
    host.settle(&mut f.runtime);
    assert!(!ticket.is_finished());
    f.runtime.collect_garbage().unwrap();
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(1));
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 102);
    let mut ticket = scheduled("successful", &[], &mut f);
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 34);
    let mut ticket = scheduled("ordinary_result", &[], &mut f);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 3);
    host.settle(&mut f.runtime);
    assert!(!ticket.is_finished(), "join does not interpret Err");
    assert_eq!(f.cancels.load(Ordering::SeqCst), 0);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(9));
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 9);
    let mut ticket = scheduled("nested_output", &[], &mut f);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 4);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(7));
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 9);
    assert!(f.runtime.take_task_event().is_none());
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn groups_right_err_cleans_sibling_before_publication_and_keeps_origin() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let future = f.invoke("fail_right", &[]);
    let cancels = f.cancels.clone();
    let selected_execution = f.start_future(&future);
    let mut ticket = f
        .runtime
        .schedule_execution(selected_execution, move |runtime, root| {
            assert_eq!(
                cancels.load(Ordering::SeqCst),
                1,
                "cleanup precedes parent result conversion"
            );
            let trace = runtime
                .result_failure(root)
                .expect("original Err origin")
                .trace;
            assert!(
                trace
                    .frames
                    .iter()
                    .any(|frame| frame.function_name.contains("reject")),
                "{trace:?}"
            );
            Ok(root.clone())
        })
        .unwrap();
    host.until(&mut f.runtime, || ticket.is_finished());
    let root = result(&mut ticket).unwrap();
    f.runtime.collect_garbage().unwrap();
    assert!(f.runtime.result_failure(&root).is_some());
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    let late = f.sent.lock().unwrap().pop().unwrap();
    assert_eq!(late.complete(Ok(99)), CompletionStatus::Stale);
    host.settle(&mut f.runtime);
    assert!(f.runtime.take_task_event().is_none());
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn groups_claim_pair_atomically_and_bound_admission() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let input = f.invoke("input", &[]);
    let mut duplicate = scheduled("duplicate", slice::from_ref(&input), &mut f);
    host.until(&mut f.runtime, || duplicate.is_finished());
    assert_eq!(
        result(&mut duplicate).unwrap_err().kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    let selected_execution = f.start_future(&input);
    let mut consumed = f
        .runtime
        .schedule_execution(selected_execution, decode)
        .unwrap();
    host.until(&mut f.runtime, || consumed.is_finished());
    assert_eq!(result(&mut consumed).unwrap(), 7);
    let other = f.invoke("input", &[]);
    let mut invalid = scheduled("supplied", &[other.clone(), input.clone()], &mut f);
    host.until(&mut f.runtime, || invalid.is_finished());
    assert_eq!(
        result(&mut invalid).unwrap_err().kind(),
        RuntimeErrorKind::ScriptTrap
    );
    let selected_execution = f.start_future(&other);
    let mut untouched = f
        .runtime
        .schedule_execution(selected_execution, decode)
        .unwrap();
    host.until(&mut f.runtime, || untouched.is_finished());
    assert_eq!(result(&mut untouched).unwrap(), 7);
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);

    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_group_depth = NonZeroUsize::new(1).unwrap();
    let mut f = Fixture::configured_source(SOURCE, config);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut ticket = scheduled("nested", &[], &mut f);
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(
        result(&mut ticket).unwrap_err().kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "nested left branch terminates before right activation"
    );
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn groups_parent_drop_drains_nested_waits_and_shares_turn_budget() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut ticket = scheduled("nested", &[], &mut f);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 3);
    host.settle(&mut f.runtime);
    f.runtime.cancel_scheduled_execution(&ticket).unwrap();
    assert_eq!(f.cancels.load(Ordering::SeqCst), 3);
    assert_eq!(
        result(&mut ticket).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    for late in f.sent.lock().unwrap().drain(..) {
        assert_eq!(late.complete(Ok(99)), CompletionStatus::Stale);
    }
    host.settle(&mut f.runtime);
    let mut ticket = scheduled("fair", &[], &mut f);
    let budget = TurnBudget {
        instructions: NonZeroUsize::new(1).unwrap(),
        quantum: NonZeroUsize::new(1).unwrap(),
        activations: NonZeroUsize::new(1).unwrap(),
    };
    for _ in 0..100 {
        if f.starts.load(Ordering::SeqCst) == 4 {
            break;
        }
        host.turn(&mut f.runtime, budget, None);
    }
    assert_eq!(f.starts.load(Ordering::SeqCst), 4);
    assert!(!ticket.is_finished());
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(5));
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 205);
}
*/
