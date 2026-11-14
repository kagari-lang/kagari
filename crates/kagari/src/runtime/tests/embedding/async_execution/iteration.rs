/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn sdk_for_await_iterator_control_contract() {
    let source = include_str!("../../../../../tests/fixtures/embedding/async/iterator-control.kgr");
    let mut f = Fixture::source(source);
    let calls = f.invoke("calls", &[]);
    let future = f.invoke("custom", slices::from_ref(&calls));
    let owner = f.start_future(&future);
    for (input, count) in [(11, 111), (12, 112)] {
        assert!(matches!(
            f.runtime.drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        assert_eq!(
            f.invoke("counts", slices::from_ref(&calls))
                .as_i32()
                .unwrap(),
            count
        );
        f.runtime.collect_garbage().unwrap();
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(7)),
            CompletionStatus::Accepted
        );
    }
    assert_eq!(f.complete(&owner).as_i32().unwrap(), 7);
    assert_eq!(
        f.invoke("counts", slices::from_ref(&calls))
            .as_i32()
            .unwrap(),
        112
    );

    let items = f.invoke("items", &[]);
    let future = f.invoke("nested", slices::from_ref(&items));
    let owner = f.start_future(&future);
    for (index, input) in [20, 109, 109, 198].into_iter().enumerate() {
        assert!(matches!(
            f.runtime.drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        if index == 0 {
            drop(f.invoke("replace", slices::from_ref(&items)));
        }
        f.runtime.collect_garbage().unwrap();
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
            CompletionStatus::Accepted
        );
    }
    assert_eq!(f.complete(&owner).as_i32().unwrap(), 42);
    drop(f.invoke("append", slices::from_ref(&items)));

    // Both nested cursors survive suspension, so a committed structural write
    // invalidates the next read without starting another native request.
    let invalidated_items = f.invoke("items", &[]);
    let future = f.invoke("nested", slices::from_ref(&invalidated_items));
    let invalidated = f.start_future(&future);
    assert!(matches!(
        f.runtime.drive(&invalidated, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let starts = f.starts.load(Ordering::SeqCst);
    drop(f.invoke("append", slices::from_ref(&invalidated_items)));
    assert_eq!(
        f.invoke("size", slices::from_ref(&invalidated_items))
            .as_u64()
            .unwrap(),
        4
    );
    f.runtime.collect_garbage().unwrap();
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
        CompletionStatus::Accepted
    );
    let DriveResult::Complete(Err(error)) = f.runtime.drive(&invalidated, slice()).unwrap() else {
        panic!("invalidated cursor must trap on resume");
    };

    assert!(
        matches!(
            error,
            ref error if error.kind() == RuntimeErrorKind::ScriptTrap
        ),
        "{error:?}"
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), starts);
    assert_eq!(
        f.invoke("size", slices::from_ref(&invalidated_items))
            .as_u64()
            .unwrap(),
        4
    );

    let future = f.invoke("trapping", slices::from_ref(&items));
    let owner = f.start_future(&future);
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(0)),
        CompletionStatus::Accepted
    );
    let DriveResult::Complete(Err(error)) = f.runtime.drive(&owner, slice()).unwrap() else {
        panic!("division trap after resume");
    };

    assert!(
        matches!(
            error,
            ref error if error.kind() == RuntimeErrorKind::ScriptTrap
        ),
        "{error:?}"
    );
    let trace = error.trace().unwrap();
    assert!(trace.frames[0].source_uri.ends_with("async-script.kgr"));
    let expected_line = u32::try_from(
        source
            .lines()
            .position(|line| line.contains("val bad"))
            .unwrap()
            + 1,
    )
    .unwrap();
    assert_eq!(trace.frames[0].line, Some(expected_line));
    drop(f.invoke("append", slices::from_ref(&items)));
    assert_eq!(f.runtime.resources().counters().current_call_depth, 0);
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn sdk_script_future_and_for_await_contract() {
    let mut f = Fixture::source(include_str!(
        "../../../../../tests/fixtures/embedding/async/script-future-iteration.kgr"
    ));
    let items = f.invoke("items", &[]);
    let future = f.invoke("total", slices::from_ref(&items));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "creation must stay cold"
    );
    let owner = f.start_future(&future);
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "queueing must stay cold"
    );
    drop(future);
    f.runtime.collect_garbage().unwrap();
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(*f.inputs.lock().unwrap(), [10]);
    drop(f.invoke("replace", slices::from_ref(&items)));
    for (reply, expected) in [(5, vec![10, 99]), (6, vec![10, 99, 30])] {
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(reply)),
            CompletionStatus::Accepted
        );
        assert!(matches!(
            f.runtime.drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(
            *f.inputs.lock().unwrap(),
            expected,
            "saved cursor must observe later replacement"
        );
        f.runtime.collect_garbage().unwrap();
    }
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(7)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.complete(&owner).as_i32().unwrap(), 18);
    drop(f.invoke("append", slices::from_ref(&items)));
    let shared = f.invoke("closures", &[]);
    assert_eq!(f.run_future(&shared).as_i32().unwrap(), 12);
    let nested = f.invoke("nested", &[]);
    let inner = f.run_future(&nested);
    let owner = f.runtime.select_program(&f.module).unwrap();
    {
        let mut cx = ConversionContext::new(&mut f.runtime, &owner).unwrap();
        assert_eq!(
            cx.import(&inner).unwrap().kind(),
            Some(ObjectKind::Native),
            "one root drive does not flatten nested Futures"
        );
    }
    assert_eq!(f.run_future(&inner).as_i32().unwrap(), 7);
    let early = f.invoke("early", &[]);
    let owner = f.start_future(&early);
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(-7)),
        CompletionStatus::Accepted
    );
    let result = f.complete(&owner);
    assert!(f.runtime.result_failure(&result).is_some());
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        4,
        "business Err skips the second RPC"
    );
    let success = f.invoke("early", &[]);
    let owner = f.start_future(&success);
    for (input, reply) in [(50, 21), (21, 42)] {
        assert!(matches!(
            f.runtime.drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(reply)),
            CompletionStatus::Accepted
        );
    }
    let result = f.complete(&owner);
    let owner = f.runtime.select_program(&f.module).unwrap();
    assert!(f.runtime.result_failure(&result).is_none());
    assert_eq!(
        ConversionContext::new(&mut f.runtime, &owner)
            .unwrap()
            .decode::<Result<i32, i32>>(&result)
            .unwrap(),
        Ok(42)
    );

    // Both terminal paths must retire the saved cursor and pending IO.
    for abandon in [false, true] {
        let items = f.invoke("items", &[]);
        let future = f.invoke("total", slices::from_ref(&items));
        let owner = f.start_future(&future);
        assert!(matches!(
            f.runtime.drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        let completion = f.sent.lock().unwrap().pop().unwrap();
        if abandon {
            drop(owner);
            assert_eq!(f.runtime.drain_retired_executions().unwrap(), 1);
        } else {
            owner.cancel();
            assert!(matches!(
                f.runtime.drive(&owner, slice()).unwrap(),
                DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::Cancelled)
            ));
        }
        assert_eq!(completion.complete(Ok(100)), CompletionStatus::Stale);
        f.runtime.collect_garbage().unwrap();
        drop(f.invoke("append", slices::from_ref(&items)));
    }
    assert_eq!(f.cancels.load(Ordering::SeqCst), 2);

    let events = f.invoke("empty", &[]);
    let future = f.invoke("construct", slices::from_ref(&events));
    assert_eq!(
        f.invoke("order", slices::from_ref(&events))
            .as_i32()
            .unwrap(),
        12
    );
    assert_eq!(f.run_future(&future).as_i32().unwrap(), 12);
    assert_eq!(
        f.invoke("size", slices::from_ref(&events))
            .as_u64()
            .unwrap(),
        2
    );

    let items = f.invoke("items", &[]);
    let future = f.invoke("lazy", slices::from_ref(&items));
    assert_eq!(
        f.invoke("size", slices::from_ref(&items)).as_u64().unwrap(),
        3
    );
    let queued = f.start_future(&future);
    queued.cancel();
    assert!(matches!(
        f.runtime.drive(&queued, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::Cancelled)
    ));
    // Cancellation before first drive does not claim the cold value.
    drop(f.run_future(&future));
    assert_eq!(
        f.invoke("size", slices::from_ref(&items)).as_u64().unwrap(),
        4
    );
    let repeated = f.start_future(&future);
    assert!(matches!(
        f.runtime.drive(&repeated, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::ScriptTrap)
    ));
    assert_eq!(
        f.invoke("size", slices::from_ref(&items)).as_u64().unwrap(),
        4
    );
}
*/
