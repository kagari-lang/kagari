//! Source factories, nested awaits and owned iteration across real native waits.
use super::{CompletionStatus, DriveResult, Fixture, Ordering, PreparedProgram, Value, slice};
use kagari::{
    BytecodeArtifact, RunResult,
    engine::EngineConfig,
    error::{EmbeddingError, RuntimeFailureKind},
};
use kagari_runtime::{gc::roots::RootedValue, session::owned::OwnedExecution};
use kagari_source::source::SourceFile;
use std::num::NonZeroUsize;

#[path = "tasks.rs"]
mod tasks;

#[path = "task_waiters.rs"]
mod task_waiters;

#[path = "task_handlers.rs"]
mod task_handlers;

#[path = "lifecycle.rs"]
mod lifecycle;

#[path = "diagnostics.rs"]
mod diagnostics;

#[path = "debugger.rs"]
mod debugger;

#[path = "conversion.rs"]
mod conversion;

impl Fixture {
    fn source(text: &str) -> Self {
        Self::configured_source(text, Default::default())
    }

    fn configured_source(text: &str, config: EngineConfig) -> Self {
        let mut fixture = Self::configured(config);
        fixture.load_source(text);
        fixture
    }

    fn load_source(&mut self, text: &str) {
        let artifact = self
            .engine
            .compile_to_artifact(
                SourceFile::new("async-script.kgr", text),
                Default::default(),
            )
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(
            &artifact.records().to_bytes(artifact.definitions()).unwrap(),
        )
        .unwrap();
        self.program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        self.module = self
            .runtime
            .load_program(&self.program, Default::default())
            .unwrap();
    }

    fn invoke(&self, entry: &str, arguments: &[Value]) -> RootedValue {
        self.call(entry, arguments).unwrap()
    }

    fn call(&self, entry: &str, arguments: &[Value]) -> RunResult<RootedValue> {
        if arguments.is_empty() {
            return self
                .runtime
                .execute(&self.module, entry, arguments, &Default::default())
                .map(|report| report.return_value);
        }
        let owner = self
            .runtime
            .start(&self.module, entry, arguments, &Default::default())?;
        for _ in 0..1000 {
            match self.runtime.drive(&owner, slice())? {
                DriveResult::Complete(result) => return result,
                DriveResult::Runnable => {}
                DriveResult::Waiting => panic!("synchronous entry waited"),
            }
        }
        panic!("bounded ordinary call");
    }

    fn value(&self, root: &RootedValue) -> Value {
        root.value(self.runtime.runtime().gc()).unwrap()
    }

    fn start_future(&self, future: &RootedValue) -> OwnedExecution {
        self.runtime
            .start_future(&self.value(future), &Default::default())
            .unwrap()
    }

    fn complete(&self, owner: &OwnedExecution) -> RootedValue {
        for _ in 0..1000 {
            match self
                .runtime
                .drive(owner, NonZeroUsize::new(1).unwrap())
                .unwrap()
            {
                DriveResult::Runnable => {
                    self.runtime.runtime().collect_garbage().unwrap();
                }
                DriveResult::Complete(value) => return value.unwrap(),
                DriveResult::Waiting => panic!("unexpected native wait"),
            }
        }
        panic!("bounded completion");
    }
}

#[test]
fn sdk_owned_future_factory_contract() {
    let f = Fixture::source(
        r#"
use test::async_sdk::request;
pub fn events() -> Vec<i32> { Vec::from([]) }
pub fn count(events:Vec<i32>) -> usize { events.len() }
async fn read() -> i32 { request(10).await }
pub fn ordinary(events:Vec<i32>) -> fn()->Future<i32> { || { events.push(1); read() } }
pub fn explicit(events:Vec<i32>) -> fn()->Future<i32> { async || { events.push(2); request(20).await } }
pub fn reentry(events:Vec<i32>) -> fn()->Future<i32> { || { val items=Vec::from([3,1,2]); items.retain(|x| { events.push(x); true }); read() } }
pub fn wrong() -> fn()->i32 { || 7 }
pub fn trapped(events:Vec<i32>) -> fn()->Future<i32> { || { events.push(3); val zero=0; val bad=1/zero; read() } }
pub fn captured() -> fn()->Future<i32> { val future=read(); || future }
async fn outer() -> Future<i32> { read() }
pub fn nested() -> fn()->Future<Future<i32>> { || outer() }
"#,
    );
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
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Complete(Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ScriptTrap,
            ..
        }))
    ));
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&events)])),
        Value::U64(3)
    );
    let owner = start(&factory);
    owner.cancel();
    assert!(matches!(
        f.runtime.drive(&owner, slice()).unwrap(),
        DriveResult::Complete(Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::Cancelled,
            ..
        }))
    ));
    assert_eq!(
        f.value(&f.invoke("count", &[f.value(&events)])),
        Value::U64(3)
    );

    let factory = f.invoke("captured", &[]);
    let first = start(&factory);
    let second = start(&factory);
    assert!(matches!(
        f.runtime.drive(&first, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert!(matches!(
        f.runtime.drive(&second, slice()).unwrap(),
        DriveResult::Complete(Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ScriptTrap,
            ..
        }))
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
        f.runtime.drive(&owner, slice()).unwrap(),
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
        f.runtime.drive(&owner, slice()).unwrap(),
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

#[test]
fn sdk_for_await_iterator_control_contract() {
    let source = r#"
use test::async_sdk::request;
use std::collections;
struct Calls { var source:i32, var into:i32, var next:i32 }
struct Range { val calls:Calls }
struct Counter { val calls:Calls }
pub fn calls() -> Calls { Calls { source:0, into:0, next:0 } }
pub fn counts(calls:Calls) -> i32 { calls.source*100+calls.into*10+calls.next }
fn make(calls:Calls) -> Range { calls.source+=1; Range { calls } }
impl Iterable for Range {
    type Item=i32;
    type Iter=Counter;
    fn iter(self) -> Counter { self.calls.into+=1; Counter { calls:self.calls } }
}
impl Iterator for Counter {
    type Item=i32;
    fn next(self) -> Option<i32> { self.calls.next+=1; Some(self.calls.next+10) }
}
pub async fn custom(calls:Calls) -> i32 {
    var total=0;
    for item in make(calls) {
        val reply=request(item).await;
        if item==11 { continue; }
        total+=reply;
        break;
    }
    total
}
pub fn items() -> Vec<i32> { Vec::from([10,20,30]) }
pub fn replace(items:Vec<i32>) { items[1]=99; }
pub fn size(items:Vec<i32>) -> usize { items.len() }
pub fn append(items:Vec<i32>) { items.push(40); }
fn view(items:Vec<i32>) -> Iterator<Item=i32> { items.map(|item| item) }
pub async fn nested(items:Vec<i32>) -> i32 {
    for outer in items {
        for inner in view(items) {
            val reply=request(outer+inner).await;
            if inner==10 { continue; }
            if outer!=10 { return reply; }
            break;
        }
    }
    0
}
pub async fn trapping(items:Vec<i32>) -> i32 {
    for item in items { val zero=request(item).await; val bad=1/zero; }
    0
}
"#;
    let f = Fixture::source(source);
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
            f.runtime.drive(&owner, slice()).unwrap(),
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
        f.runtime.drive(&invalidated, slice()).unwrap(),
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
    let DriveResult::Complete(Err(error)) = f.runtime.drive(&invalidated, slice()).unwrap() else {
        panic!("invalidated cursor must trap on resume");
    };
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
    let f = Fixture::source(
        r#"
use test::async_sdk::request;
async fn identity<T>(value: T) -> T { value }
fn apply<F:Fn(i32)->Future<i32>>(f:F, value:i32) -> Future<i32> { f(value) }
pub async fn total(items: Vec<i32>) -> i32 {
    val fetch: fn(i32) -> Future<i32> = async |item| request(identity(item).await).await;
    var sum = 0;
    for item in items { sum += apply(fetch, item).await; }
    sum
}
pub fn items() -> Vec<i32> { Vec::from([10, 20, 30]) }
pub fn replace(items: Vec<i32>) { items[1] = 99; }
pub fn append(items: Vec<i32>) { items.push(40); }
pub fn size(items: Vec<i32>) -> usize { items.len() }
pub async fn lazy(items: Vec<i32>) { items.push(50); }
fn record(events:Vec<i32>, value:i32) -> i32 { events.push(value); value }
async fn ordered(first:i32, second:i32) -> i32 { first*10+second }
pub fn construct(events:Vec<i32>) -> Future<i32> { ordered(record(events,1),record(events,2)) }
pub fn empty() -> Vec<i32> { Vec::from([]) }
pub fn order(events:Vec<i32>) -> i32 { events[0]*10+events[1] }
pub async fn closures() -> i32 {
    var count = 0;
    val next = async || { count += 1; identity(count).await };
    val first = next();
    val second = next();
    first.await * 10 + second.await
}
pub async fn nested() -> Future<i32> { identity(7) }
pub async fn early() -> Result<i32, i32> {
    val first=request(50).await;
    val result:Result<i32,i32> = if first<0 { Err(first) } else { Ok(first) };
    val value=identity(result).await?;
    Ok(request(value).await)
}
"#,
    );
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
        f.runtime.drive(&owner, slice()).unwrap(),
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
                DriveResult::Complete(Err(EmbeddingError::Runtime {
                    kind: RuntimeFailureKind::Cancelled,
                    ..
                }))
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
        f.runtime.drive(&queued, slice()).unwrap(),
        DriveResult::Complete(Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::Cancelled,
            ..
        }))
    ));
    // Cancellation before first drive does not claim the cold value.
    drop(f.complete(&f.start_future(&future)));
    assert_eq!(
        f.value(&f.invoke("size", &[f.value(&items)])),
        Value::U64(4)
    );
    let repeated = f.start_future(&future);
    assert!(matches!(
        f.runtime.drive(&repeated, slice()).unwrap(),
        DriveResult::Complete(Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ScriptTrap,
            ..
        }))
    ));
    assert_eq!(
        f.value(&f.invoke("size", &[f.value(&items)])),
        Value::U64(4)
    );
}
