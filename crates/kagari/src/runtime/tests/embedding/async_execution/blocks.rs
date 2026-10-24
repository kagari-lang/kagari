//! Async blocks share decoded Future execution, capture roots and one-shot state.
use super::{Fixture, Value, slice};
use crate::error::{EmbeddingError, RuntimeFailureKind};
use kagari_runtime::native::completion::CompletionStatus;
use kagari_vm::vm::owned::DriveResult;
use std::sync::atomic::Ordering;

#[test]
fn sdk_async_block_capture_and_control_flow_contract() {
    let f = Fixture::source(include_str!(
        "../../../../../tests/fixtures/embedding/async/blocks-capture-control.kgr"
    ));
    for (entry, expected) in [("captures", 155), ("boundaries", 339), ("propagation", 72)] {
        let future = f.invoke(entry, &[]);
        assert_eq!(
            f.value(&f.complete(&f.start_future(&future))),
            Value::I32(expected)
        );
    }
    let unit = f.invoke("unit", &[]);
    assert_eq!(f.value(&f.complete(&f.start_future(&unit))), Value::Unit);
    let events = f.invoke("events", &[]);
    drop(f.invoke("discard", &[f.value(&events)]));
    let outer = f.invoke("nested", &[f.value(&events)]);
    let inner = f.complete(&f.start_future(&outer));
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&events)])),
        Value::U64(0)
    );
    assert_eq!(
        f.value(&f.complete(&f.start_future(&inner))),
        Value::I32(42)
    );
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&events)])),
        Value::U64(1)
    );
    let repeated = f.start_future(&inner);
    assert!(matches!(
        f.runtime.backend().drive(&repeated, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(EmbeddingError::from(error.clone()), EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ScriptTrap,
            ..
        })
    ));
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&events)])),
        Value::U64(1)
    );
}

#[test]
fn sdk_async_block_native_wait_retains_captures() {
    let f = Fixture::source(
        r#"
use test::async_sdk::request;
pub fn cold() -> Future<i32> {
    val values = Vec::from([40]);
    async { val response = request(values[0]).await; values[0] + response }
}
"#,
    );
    let future = f.invoke("cold", &[]);
    f.runtime.runtime().collect_garbage().unwrap();
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    let owner = f.start_future(&future);
    drop(future);
    assert!(matches!(
        f.runtime.backend().drive(&owner, slice()).unwrap(),
        DriveResult::Waiting
    ));
    f.runtime.runtime().collect_garbage().unwrap();
    assert_eq!(*f.inputs.lock().unwrap(), [40]);
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(2)),
        CompletionStatus::Accepted
    );
    assert_eq!(f.value(&f.complete(&owner)), Value::I32(42));
    drop(owner);
    f.runtime.runtime().collect_garbage().unwrap();
    assert_eq!(f.runtime.runtime().gc().active_roots(), 0);
}
