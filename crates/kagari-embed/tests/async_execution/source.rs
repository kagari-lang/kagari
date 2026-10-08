//! Source factories, nested awaits and owned iteration across real native waits.
use super::{CompletionStatus, DriveResult, Fixture, Ordering, PreparedProgram, Value, slice};
use kagari_embed::{
    BytecodeArtifact, RunResult,
    error::{EmbeddingError, RuntimeFailureKind},
};
use kagari_runtime::{gc::roots::RootedValue, session::owned::OwnedExecution};
use kagari_source::source::SourceFile;
use std::num::NonZeroUsize;

impl Fixture {
    fn source(text: &str) -> Self {
        let mut fixture = Self::new();
        let artifact = fixture
            .engine
            .compile_to_artifact(
                SourceFile::new("async-script.kgr", text),
                Default::default(),
            )
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        fixture.program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        fixture.module = fixture
            .runtime
            .load_program(&fixture.program, Default::default())
            .unwrap();
        fixture
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
fn sdk_script_future_and_for_await_contract() {
    let f = Fixture::source(
        r#"
use test::async_sdk::request;
async fn identity<T>(value: T) -> T { value }
async fn total(items: Vec<i32>) -> i32 {
    val fetch: fn(i32) -> Future<i32> = async |item| request(identity(item).await).await;
    var sum = 0;
    for item in items { sum += fetch(item).await; }
    sum
}
fn items() -> Vec<i32> { [10, 20, 30] }
fn replace(items: Vec<i32>) { items[1] = 99; }
fn append(items: Vec<i32>) { items.push(40); }
fn size(items: Vec<i32>) -> usize { items.len() }
async fn lazy(items: Vec<i32>) { items.push(50); }
async fn closures() -> i32 {
    var count = 0;
    val next = async || { count += 1; identity(count).await };
    val first = next();
    val second = next();
    first.await * 10 + second.await
}
async fn nested() -> Future<i32> { identity(7) }
async fn early() -> Result<i32, i32> {
    val result: Result<i32, i32> = Err(7);
    val value = identity(result).await?;
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
    let error = f.call("append", &[f.value(&items)]).unwrap_err();
    assert!(
        matches!(error, EmbeddingError::Runtime { ref message, .. } if message.contains("iteration")),
        "structural mutation while the loop waits: {error:?}"
    );
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
    let result = f.complete(&f.start_future(&early));
    assert!(
        f.runtime
            .runtime()
            .result_failure(&f.value(&result))
            .is_some()
    );
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        3,
        "business Err skips native IO"
    );

    // Both terminal paths must release the cursor lease and retire pending IO.
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
