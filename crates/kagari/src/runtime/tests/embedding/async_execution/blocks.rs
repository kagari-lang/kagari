//! Async blocks share decoded Future execution, capture roots and one-shot state.
use super::{Fixture, Value, slice};
use crate::{
    error::{EmbeddingError, RuntimeFailureKind},
    runtime::owned::DriveResult,
};
use kagari_runtime::native::completion::CompletionStatus;
use std::sync::atomic::Ordering;

#[test]
fn sdk_async_block_capture_and_control_flow_contract() {
    let f = Fixture::source(
        r#"
fn identity<T>(value: T) -> Future<T> { async { value } }
pub fn events() -> Vec<i32> { Vec::from([]) }
pub fn count(events: Vec<i32>) -> u64 { events.len() }
pub fn cold(events: Vec<i32>) -> Future<i32> { async { events.push(1); 42 } }
pub fn discard(events: Vec<i32>) { async { events.push(99); }; }
pub fn nested(events: Vec<i32>) -> Future<Future<i32>> { async { cold(events) } }
pub async fn captures() -> i32 {
    var count = 1;
    val scalar = 4;
    val first = async { count += scalar; count };
    val make = || async { count += 1; count };
    val second = make();
    count = 10;
    first.await * 10 + second.await
}
pub async fn boundaries() -> i32 {
    val first = async { val f = || { return 3; }; return f(); }.await;
    var sum = 0;
    for value in [1, 2] {
        sum += async {
            var n = 0;
            while n < value { n += 1; continue; }
            val result = loop { break identity(n).await; };
            result
        }.await;
    }
    val nested = async { async { return 4; }.await + 5 }.await;
    first * 100 + sum * 10 + nested
}
pub async fn propagation() -> i32 {
    val failure: Future<Result<i32, i32>> = async {
        val result: Result<i32, i32> = Err(7);
        val value = result?;
        Ok(value + 100)
    };
    val absent: Future<Option<i32>> = async {
        val value: Option<i32> = None;
        Some(value?)
    };
    val a = match failure.await { Ok(_) => 0, Err(n) => n };
    val b = match absent.await { Some(_) => 0, None => 2 };
    a * 10 + b
}
pub fn unit() -> Future<()> { async {} }
"#,
    );
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
        f.runtime.drive(&repeated, slice()).unwrap(),
        DriveResult::Complete(Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ScriptTrap,
            ..
        }))
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
        f.runtime.drive(&owner, slice()).unwrap(),
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
