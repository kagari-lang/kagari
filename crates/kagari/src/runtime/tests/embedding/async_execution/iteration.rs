use super::{Fixture, Value, slice};
use crate::error::{EmbeddingError, RuntimeFailureKind};
use kagari_runtime::native::completion::CompletionStatus;
use kagari_vm::vm::owned::DriveResult;
use std::sync::atomic::Ordering;

#[test]
fn sdk_for_await_iterator_control_contract() {
    let source = include_str!("../../../../../tests/fixtures/embedding/async/iterator-control.kgr");
    let f = Fixture::source(source);
    let calls = f.invoke("calls", &[]);
    let future = f.invoke("custom", &[f.value(&calls)]);
    let owner = f.start_future(&future);
    for (input, count) in [(11, 111), (12, 112)] {
        assert!(matches!(
            f.runtime.backend().drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        assert_eq!(
            f.value(&f.invoke("counts", &[f.value(&calls)])),
            Value::I32(count)
        );
        f.runtime.runtime().collect_garbage().unwrap();
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(7)),
            CompletionStatus::Accepted
        );
    }
    assert_eq!(f.value(&f.complete(&owner)), Value::I32(7));
    assert_eq!(
        f.value(&f.invoke("counts", &[f.value(&calls)])),
        Value::I32(112)
    );

    let items = f.invoke("items", &[]);
    let future = f.invoke("nested", &[f.value(&items)]);
    let owner = f.start_future(&future);
    for (index, input) in [20, 109, 109, 198].into_iter().enumerate() {
        assert!(matches!(
            f.runtime.backend().drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        if index == 0 {
            drop(f.invoke("replace", &[f.value(&items)]));
        }
        f.runtime.runtime().collect_garbage().unwrap();
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
            CompletionStatus::Accepted
        );
    }
    assert_eq!(f.value(&f.complete(&owner)), Value::I32(42));
    drop(f.invoke("append", &[f.value(&items)]));

    // Both nested cursors survive suspension, so a committed structural write
    // invalidates the next read without starting another native request.
    let invalidated_items = f.invoke("items", &[]);
    let future = f.invoke("nested", &[f.value(&invalidated_items)]);
    let invalidated = f.start_future(&future);
    assert!(matches!(
        f.runtime.backend().drive(&invalidated, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let starts = f.starts.load(Ordering::SeqCst);
    drop(f.invoke("append", &[f.value(&invalidated_items)]));
    assert_eq!(
        f.value(&f.invoke("size", &[f.value(&invalidated_items)])),
        Value::U64(4)
    );
    f.runtime.runtime().collect_garbage().unwrap();
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
        CompletionStatus::Accepted
    );
    let DriveResult::Complete(Err(error)) =
        f.runtime.backend().drive(&invalidated, slice()).unwrap()
    else {
        panic!("invalidated cursor must trap on resume");
    };
    let error = EmbeddingError::from(error);
    assert!(
        matches!(
            error,
            EmbeddingError::Runtime {
                kind: RuntimeFailureKind::ScriptTrap,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), starts);
    assert_eq!(
        f.value(&f.invoke("size", &[f.value(&invalidated_items)])),
        Value::U64(4)
    );

    let future = f.invoke("trapping", &[f.value(&items)]);
    let owner = f.start_future(&future);
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(0)),
        CompletionStatus::Accepted
    );
    let DriveResult::Complete(Err(error)) = f.runtime.backend().drive(&owner, slice()).unwrap()
    else {
        panic!("division trap after resume");
    };
    let error = EmbeddingError::from(error);
    assert!(
        matches!(
            error,
            EmbeddingError::Runtime {
                kind: RuntimeFailureKind::ScriptTrap,
                ..
            }
        ),
        "{error:?}"
    );
    let trace = error.error_trace().unwrap();
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
    assert_eq!(
        f.runtime
            .runtime()
            .resources()
            .counters()
            .current_call_depth,
        0
    );
}

#[test]
fn sdk_script_future_and_for_await_contract() {
    let f = Fixture::source(include_str!(
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
    f.runtime.runtime().collect_garbage().unwrap();
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
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
            f.runtime.backend().drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(
            *f.inputs.lock().unwrap(),
            expected,
            "saved cursor must observe later replacement"
        );
        f.runtime.runtime().collect_garbage().unwrap();
    }
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(7)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.value(&f.complete(&owner)), Value::I32(18));
    drop(f.invoke("append", &[f.value(&items)]));
    let shared = f.invoke("closures", &[]);
    assert_eq!(
        f.value(&f.complete(&f.start_future(&shared))),
        Value::I32(12)
    );
    let nested = f.invoke("nested", &[]);
    let inner = f.complete(&f.start_future(&nested));
    assert!(
        matches!(f.value(&inner), Value::GcHandle(_)),
        "one root drive does not flatten nested Futures"
    );
    assert_eq!(f.value(&f.complete(&f.start_future(&inner))), Value::I32(7));
    let early = f.invoke("early", &[]);
    let owner = f.start_future(&early);
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(-7)),
        CompletionStatus::Accepted
    );
    let result = f.complete(&owner);
    assert!(
        f.runtime
            .runtime()
            .result_failure(&f.value(&result))
            .is_some()
    );
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        4,
        "business Err skips the second RPC"
    );
    let success = f.invoke("early", &[]);
    let owner = f.start_future(&success);
    for (input, reply) in [(50, 21), (21, 42)] {
        assert!(matches!(
            f.runtime.backend().drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(reply)),
            CompletionStatus::Accepted
        );
    }
    let result = f.complete(&owner);
    assert!(
        f.runtime
            .runtime()
            .result_failure(&f.value(&result))
            .is_none()
    );
    let Value::Enum(id) = f.value(&result) else {
        panic!("business Result");
    };
    assert_eq!(
        f.runtime.runtime().gc().enum_snapshot(id).unwrap().fields,
        [Value::I32(42)]
    );

    // Both terminal paths must retire the saved cursor and pending IO.
    for abandon in [false, true] {
        let items = f.invoke("items", &[]);
        let future = f.invoke("total", &[f.value(&items)]);
        let owner = f.start_future(&future);
        assert!(matches!(
            f.runtime.backend().drive(&owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
        let completion = f.sent.lock().unwrap().pop().unwrap();
        if abandon {
            drop(owner);
            assert_eq!(f.runtime.runtime().drain_retired_executions().unwrap(), 1);
        } else {
            owner.cancel();
            assert!(matches!(
                f.runtime.backend().drive(&owner, slice()).unwrap(),
                DriveResult::Complete(Err(error)) if matches!(EmbeddingError::from(error.clone()), EmbeddingError::Runtime {
                    kind: RuntimeFailureKind::Cancelled,
                    ..
                })
            ));
        }
        assert_eq!(completion.complete(Ok(100)), CompletionStatus::Stale);
        f.runtime.runtime().collect_garbage().unwrap();
        drop(f.invoke("append", &[f.value(&items)]));
    }
    assert_eq!(f.cancels.load(Ordering::SeqCst), 2);

    let events = f.invoke("empty", &[]);
    let future = f.invoke("construct", &[f.value(&events)]);
    assert_eq!(
        f.value(&f.invoke("order", &[f.value(&events)])),
        Value::I32(12)
    );
    assert_eq!(
        f.value(&f.complete(&f.start_future(&future))),
        Value::I32(12)
    );
    assert_eq!(
        f.value(&f.invoke("size", &[f.value(&events)])),
        Value::U64(2)
    );

    let items = f.invoke("items", &[]);
    let future = f.invoke("lazy", &[f.value(&items)]);
    assert_eq!(
        f.value(&f.invoke("size", &[f.value(&items)])),
        Value::U64(3)
    );
    let queued = f.start_future(&future);
    queued.cancel();
    assert!(matches!(
        f.runtime.backend().drive(&queued, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(EmbeddingError::from(error.clone()), EmbeddingError::Runtime {
            kind: RuntimeFailureKind::Cancelled,
            ..
        })
    ));
    // Cancellation before first drive does not claim the cold value.
    drop(f.complete(&f.start_future(&future)));
    assert_eq!(
        f.value(&f.invoke("size", &[f.value(&items)])),
        Value::U64(4)
    );
    let repeated = f.start_future(&future);
    assert!(matches!(
        f.runtime.backend().drive(&repeated, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(EmbeddingError::from(error.clone()), EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ScriptTrap,
            ..
        })
    ));
    assert_eq!(
        f.value(&f.invoke("size", &[f.value(&items)])),
        Value::U64(4)
    );
}
