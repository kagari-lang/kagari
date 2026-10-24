use super::{Fixture, Value, slice};
use crate::error::{EmbeddingError, RuntimeFailureKind};
use kagari_runtime::{gc::roots::RootedValue, native::completion::CompletionStatus};
use kagari_vm::vm::owned::DriveResult;
use std::{num::NonZeroUsize, sync::atomic::Ordering};

#[test]
fn sdk_owned_future_factory_contract() {
    let f = Fixture::source(include_str!(
        "../../../../../tests/fixtures/embedding/async/owned-factory.kgr"
    ));
    let events = f.invoke("events", &[]);
    let start = |factory: &RootedValue| {
        f.runtime
            .runtime()
            .start_owned_factory(&f.value(factory), Default::default())
            .unwrap()
    };
    for (entry, expected) in [("ordinary", 1), ("explicit", 2)] {
        let factory = f.invoke(entry, &[f.value(&events)]);
        let owner = start(&factory);
        drop(factory);
        assert_eq!(f.starts.load(Ordering::SeqCst), expected - 1);
        assert_eq!(
            f.value(&f.invoke("count", &[f.value(&events)])),
            Value::U64((expected - 1) as u64)
        );
        let mut waited = false;
        for _ in 0..1000 {
            f.runtime.runtime().collect_garbage().unwrap();
            match f
                .runtime
                .backend()
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
            f.value(&f.invoke("count", &[f.value(&events)])),
            Value::U64(expected as u64)
        );
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
            CompletionStatus::Accepted
        );
        assert_eq!(f.value(&f.complete(&owner)), Value::I32(42));
        assert_eq!(
            f.value(&f.invoke("count", &[f.value(&events)])),
            Value::U64(expected as u64)
        );
    }
    let wrong = f.invoke("wrong", &[]);
    assert!(
        f.runtime
            .runtime()
            .start_owned_factory(&f.value(&wrong), Default::default())
            .is_err()
    );
    let factory = f.invoke("trapped", &[f.value(&events)]);
    let owner = start(&factory);
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(EmbeddingError::from(error.clone()), EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ScriptTrap,
            ..
        })
    ));
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&events)])),
        Value::U64(3)
    );
    let owner = start(&factory);
    owner.cancel();
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(EmbeddingError::from(error.clone()), EmbeddingError::Runtime {
            kind: RuntimeFailureKind::Cancelled,
            ..
        })
    ));
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&events)])),
        Value::U64(3)
    );

    let factory = f.invoke("captured", &[]);
    let first = start(&factory);
    let second = start(&factory);
    assert!(matches!(
        f.runtime.backend().drive(&first, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert!(matches!(
        f.runtime.backend().drive(&second, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(EmbeddingError::from(error.clone()), EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ScriptTrap,
            ..
        })
    ));
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(9)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.value(&f.complete(&first)), Value::I32(9));
    let factory = f.invoke("nested", &[]);
    let inner = f.complete(&start(&factory));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        3,
        "factory driving does not flatten its output"
    );
    let owner = f.start_future(&inner);
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(8)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.value(&f.complete(&owner)), Value::I32(8));
    let events = f.invoke("events", &[]);
    let factory = f.invoke("reentry", &[f.value(&events)]);
    let owner = start(&factory);
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&events)])),
        Value::U64(3)
    );
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(5)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.value(&f.complete(&owner)), Value::I32(5));
}
