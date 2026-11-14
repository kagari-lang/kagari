/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn sdk_owned_future_factory_contract() {
    let mut f = Fixture::source(include_str!(
        "../../../../../tests/fixtures/embedding/async/owned-factory.kgr"
    ));
    let events = f.invoke("events", &[]);
    let start = |factory: &RootedValue, f: &mut Fixture| {
        f.runtime
            .start_owned_factory(factory, Default::default())
            .unwrap()
    };
    for (entry, expected) in [("ordinary", 1), ("explicit", 2)] {
        let factory = f.invoke(entry, slices::from_ref(&events));
        let owner = start(&factory, &mut f);
        drop(factory);
        assert_eq!(f.starts.load(Ordering::SeqCst), expected - 1);
        assert_eq!(
            f.invoke("count", slices::from_ref(&events))
                .as_u64()
                .unwrap(),
            (expected - 1) as u64
        );
        let mut waited = false;
        for _ in 0..1000 {
            f.runtime.collect_garbage().unwrap();
            match f
                .runtime
                .drive(&owner, NonZeroUsize::new(1).unwrap())
                .unwrap()
            {
                DriveResult::Runnable => {}
                DriveResult::Waiting => {
                    waited = true;
                    break;
                }
                DriveResult::Complete(value) => panic!("premature completion: {value:?}"),
            }
        }
        assert!(waited);
        assert_eq!(f.starts.load(Ordering::SeqCst), expected);
        assert_eq!(
            f.invoke("count", slices::from_ref(&events))
                .as_u64()
                .unwrap(),
            expected as u64
        );
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
            CompletionStatus::Accepted
        );
        assert_eq!(f.complete(&owner).as_i32().unwrap(), 42);
        assert_eq!(
            f.invoke("count", slices::from_ref(&events))
                .as_u64()
                .unwrap(),
            expected as u64
        );
    }
    let wrong = f.invoke("wrong", &[]);
    assert!(
        f.runtime
            .start_owned_factory(&wrong, Default::default())
            .is_err()
    );
    let factory = f.invoke("trapped", slices::from_ref(&events));
    let owner = start(&factory, &mut f);
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::ScriptTrap)
    ));
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);
    assert_eq!(
        f.invoke("count", slices::from_ref(&events))
            .as_u64()
            .unwrap(),
        3
    );
    let owner = start(&factory, &mut f);
    owner.cancel();
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::Cancelled)
    ));
    assert_eq!(
        f.invoke("count", slices::from_ref(&events))
            .as_u64()
            .unwrap(),
        3
    );

    let factory = f.invoke("captured", &[]);
    let first = start(&factory, &mut f);
    let second = start(&factory, &mut f);
    assert!(matches!(
        f.runtime.drive(&first, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert!(matches!(
        f.runtime.drive(&second, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::ScriptTrap)
    ));
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(9)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.complete(&first).as_i32().unwrap(), 9);
    let factory = f.invoke("nested", &[]);
    let execution = start(&factory, &mut f);
    let inner = f.complete(&execution);
    drop(execution);
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        3,
        "factory driving does not flatten its output"
    );
    let owner = f.start_future(&inner);
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(8)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.complete(&owner).as_i32().unwrap(), 8);
    let events = f.invoke("events", &[]);
    let factory = f.invoke("reentry", slices::from_ref(&events));
    let owner = start(&factory, &mut f);
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.invoke("count", slices::from_ref(&events))
            .as_u64()
            .unwrap(),
        3
    );
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(5)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.complete(&owner).as_i32().unwrap(), 5);
}
*/
