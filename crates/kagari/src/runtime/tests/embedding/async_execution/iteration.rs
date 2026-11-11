use super::{Fixture, Value, slice};
use kagari_runtime::error::RuntimeErrorKind;
use kagari_runtime::execution::owned::DriveResult;
use kagari_runtime::native::completion::CompletionStatus;
use std::sync::atomic::Ordering;

#[test]
fn sdk_for_await_iterator_control_contract() {
    let source = include_str!("../../../../../tests/fixtures/embedding/async/iterator-control.kgr");
    let mut f = Fixture::source(source);
    let calls = f.invoke("calls", &[]);
    let future = f.invoke("custom", &[f.value(&calls)]);
    let owner = f.start_future(&future);
    for (input, count) in [(11, 111), (12, 112)] {
        assert!(matches!(
            f.runtime.drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        assert_eq!(
            f.invoke("counts", &[f.value(&calls)])
                .value(f.runtime.gc())
                .unwrap(),
            Value::I32(count)
        );
        f.runtime.collect_garbage().unwrap();
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(7)),
            CompletionStatus::Accepted
        );
    }
    assert_eq!(
        f.complete(&owner).value(f.runtime.gc()).unwrap(),
        Value::I32(7)
    );
    assert_eq!(
        f.invoke("counts", &[f.value(&calls)])
            .value(f.runtime.gc())
            .unwrap(),
        Value::I32(112)
    );

    let items = f.invoke("items", &[]);
    let future = f.invoke("nested", &[f.value(&items)]);
    let owner = f.start_future(&future);
    for (index, input) in [20, 109, 109, 198].into_iter().enumerate() {
        assert!(matches!(
            f.runtime.drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        if index == 0 {
            drop(f.invoke("replace", &[f.value(&items)]));
        }
        f.runtime.collect_garbage().unwrap();
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
            CompletionStatus::Accepted
        );
    }
    assert_eq!(
        f.complete(&owner).value(f.runtime.gc()).unwrap(),
        Value::I32(42)
    );
    drop(f.invoke("append", &[f.value(&items)]));

    // Both nested cursors survive suspension, so a committed structural write
    // invalidates the next read without starting another native request.
    let invalidated_items = f.invoke("items", &[]);
    let future = f.invoke("nested", &[f.value(&invalidated_items)]);
    let invalidated = f.start_future(&future);
    assert!(matches!(
        f.runtime.drive(&invalidated, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let starts = f.starts.load(Ordering::SeqCst);
    drop(f.invoke("append", &[f.value(&invalidated_items)]));
    assert_eq!(
        f.invoke("size", &[f.value(&invalidated_items)])
            .value(f.runtime.gc())
            .unwrap(),
        Value::U64(4)
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
        f.invoke("size", &[f.value(&invalidated_items)])
            .value(f.runtime.gc())
            .unwrap(),
        Value::U64(4)
    );

    let future = f.invoke("trapping", &[f.value(&items)]);
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
    drop(f.invoke("append", &[f.value(&items)]));
    assert_eq!(f.runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn sdk_script_future_and_for_await_contract() {
    let mut f = Fixture::source(include_str!(
        "../../../../../tests/fixtures/embedding/async/script-future-iteration.kgr"
    ));
    let items = f.invoke("items", &[]);
    let future = f.invoke("total", &[f.value(&items)]);
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
    drop(f.invoke("replace", &[f.value(&items)]));
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
    assert_eq!(
        f.complete(&owner).value(f.runtime.gc()).unwrap(),
        Value::I32(18)
    );
    drop(f.invoke("append", &[f.value(&items)]));
    let shared = f.invoke("closures", &[]);
    assert_eq!(
        f.run_future(&shared).value(f.runtime.gc()).unwrap(),
        Value::I32(12)
    );
    let nested = f.invoke("nested", &[]);
    let inner = f.run_future(&nested);
    assert!(
        matches!(f.value(&inner), Value::GcHandle(_)),
        "one root drive does not flatten nested Futures"
    );
    assert_eq!(
        f.run_future(&inner).value(f.runtime.gc()).unwrap(),
        Value::I32(7)
    );
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
    assert!(f.runtime.result_failure(&f.value(&result)).is_some());
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
    assert!(f.runtime.result_failure(&f.value(&result)).is_none());
    let Value::Enum(id) = f.value(&result) else {
        panic!("business Result");
    };
    assert_eq!(
        f.runtime.gc().enum_snapshot(id).unwrap().fields,
        [Value::I32(42)]
    );

    // Both terminal paths must retire the saved cursor and pending IO.
    for abandon in [false, true] {
        let items = f.invoke("items", &[]);
        let future = f.invoke("total", &[f.value(&items)]);
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
        drop(f.invoke("append", &[f.value(&items)]));
    }
    assert_eq!(f.cancels.load(Ordering::SeqCst), 2);

    let events = f.invoke("empty", &[]);
    let future = f.invoke("construct", &[f.value(&events)]);
    assert_eq!(
        f.invoke("order", &[f.value(&events)])
            .value(f.runtime.gc())
            .unwrap(),
        Value::I32(12)
    );
    assert_eq!(
        f.run_future(&future).value(f.runtime.gc()).unwrap(),
        Value::I32(12)
    );
    assert_eq!(
        f.invoke("size", &[f.value(&events)])
            .value(f.runtime.gc())
            .unwrap(),
        Value::U64(2)
    );

    let items = f.invoke("items", &[]);
    let future = f.invoke("lazy", &[f.value(&items)]);
    assert_eq!(
        f.invoke("size", &[f.value(&items)])
            .value(f.runtime.gc())
            .unwrap(),
        Value::U64(3)
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
        f.invoke("size", &[f.value(&items)])
            .value(f.runtime.gc())
            .unwrap(),
        Value::U64(4)
    );
    let repeated = f.start_future(&future);
    assert!(matches!(
        f.runtime.drive(&repeated, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::ScriptTrap)
    ));
    assert_eq!(
        f.invoke("size", &[f.value(&items)])
            .value(f.runtime.gc())
            .unwrap(),
        Value::U64(4)
    );
}
