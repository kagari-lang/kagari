//! Cancellation during native output conversion must precede continuation publication.
use super::{Fixture, Value};
use kagari_common::cancellation::CancellationToken;
use kagari_embed::context::ExecutionContext;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        completion::CompletionStatus,
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        future::NativeStart,
        registration::FunctionSpec,
        types::Type,
    },
    task::{CancellationCause, TaskId, control::TaskScopeOwner, drive::TaskDriveResult},
};
use std::{
    num::NonZeroUsize,
    slice,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

const SOURCE: &str = r#"
use test::converted_reply::read;
fn items() -> Vec<i32> { Vec::from([5]) }
fn push(items: Vec<i32>) { items.push(9); }
fn first(items: Vec<i32>) -> i32 { items[0] }
fn factory(items: Vec<i32>) -> fn()->Future<i32> {
    async || {
        for item in items {
            val reply = read().await;
            items[0] = reply[0] + item;
        }
        items[0]
    }
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    BeforeConversion,
    DuringConversion,
    ConversionFault,
    Success,
}

struct Reply {
    boundary: Boundary,
    cancellation: CancellationToken,
    encoded: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}

impl KagariType for Reply {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Vec::<i32>::kagari_type(catalog)
    }
}

impl IntoKagari for Reply {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        self.encoded.fetch_add(1, Ordering::SeqCst);
        let value = cx.encode_value(expected, vec![7_i32])?;
        cx.runtime().collect_garbage()?;
        match self.boundary {
            Boundary::DuringConversion => self.cancellation.cancel(),
            Boundary::ConversionFault => {
                return Err(cx
                    .runtime()
                    .quarantine_execution_invariant("invalid converter state"));
            }
            Boundary::BeforeConversion | Boundary::Success => {}
        }
        // Deliberately return without polling: the boundary must reject publication.
        Ok(value)
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

fn drive(f: &Fixture, scope: &TaskScopeOwner, task: TaskId) -> TaskDriveResult {
    for _ in 0..1000 {
        let result = f
            .runtime
            .drive_task(scope, task, NonZeroUsize::new(1).unwrap())
            .unwrap();
        if result != TaskDriveResult::Runnable {
            return result;
        }
        f.runtime.runtime().collect_garbage().unwrap();
    }
    panic!("bounded conversion driver");
}

#[test]
fn async_output_publication_contract() {
    for boundary in [
        Boundary::BeforeConversion,
        Boundary::DuringConversion,
        Boundary::ConversionFault,
        Boundary::Success,
    ] {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicUsize::new(0));
        let encoded = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut f = Fixture::with_provider(Default::default(), |engine| {
            let send = sent.clone();
            let cancelled = cancelled.clone();
            let mut module = ModuleBuilder::new("test::converted_reply", engine.declarations());
            module
                .add_async_function::<(), Reply>(
                    FunctionSpec::new("read"),
                    move |(), completion| {
                        send.lock().unwrap().push(completion);
                        let cancelled = cancelled.clone();
                        Ok(NativeStart::cancellable(move || {
                            cancelled.fetch_add(1, Ordering::SeqCst);
                        }))
                    },
                )
                .unwrap();
            engine.install(module.finish().unwrap()).unwrap();
        });
        f.load_source(SOURCE);
        let runtime = f.runtime.runtime();
        let context = ExecutionContext::default();
        let scope = f
            .runtime
            .create_task_scope(&f.module, &context, Arc::new(|_| Ok(())))
            .unwrap();
        let items = f.invoke("items", &[]);
        let factory = f.invoke("factory", &[f.value(&items)]);
        let handle = runtime
            .spawn_task(&f.value(scope.capability()), &f.value(&factory))
            .unwrap()
            .unwrap();
        drop(factory);
        let task = runtime.task_id(&f.value(&handle)).unwrap();
        assert_eq!(drive(&f, &scope, task), TaskDriveResult::Waiting);
        let reply = Reply {
            boundary,
            cancellation: context.cancellation.clone(),
            encoded: encoded.clone(),
            dropped: dropped.clone(),
        };
        let completion = sent.lock().unwrap().pop().unwrap();
        let late = completion.clone();
        thread::spawn(move || {
            assert_eq!(completion.complete(Ok(reply)), CompletionStatus::Accepted);
        })
        .join()
        .unwrap();
        assert_eq!(
            encoded.load(Ordering::SeqCst),
            0,
            "completion cannot convert on a worker"
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        if boundary == Boundary::BeforeConversion {
            context.cancellation.cancel();
        }
        assert_eq!(drive(&f, &scope, task), TaskDriveResult::Complete);
        let report = runtime.take_task_report(&scope, task).unwrap().unwrap();
        assert!(runtime.take_task_report(&scope, task).unwrap().is_none());
        if boundary == Boundary::Success {
            assert_eq!(f.value(&report.outcome.unwrap()), Value::I32(12));
        } else {
            let failure = report.outcome.unwrap_err();
            assert_eq!(failure.source_task, task);
            if boundary == Boundary::ConversionFault {
                assert_eq!(failure.error.kind(), RuntimeErrorKind::EngineFault);
                assert!(runtime.is_quarantined());
            } else {
                assert_eq!(failure.error.kind(), RuntimeErrorKind::Cancelled);
                assert_eq!(failure.cancellation, Some(CancellationCause::ScopeClose));
                assert_eq!(
                    f.value(&f.invoke("first", &[f.value(&items)])),
                    Value::I32(5)
                );
            }
        }
        assert_eq!(
            encoded.load(Ordering::SeqCst),
            usize::from(boundary != Boundary::BeforeConversion)
        );
        assert_eq!(
            cancelled.load(Ordering::SeqCst),
            usize::from(boundary == Boundary::BeforeConversion)
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(
            late.complete(Err(RuntimeError::module_validation("late"))),
            CompletionStatus::Stale
        );
        if boundary != Boundary::ConversionFault {
            assert!(!runtime.is_quarantined());
            let _ = f.invoke("push", slice::from_ref(&f.value(&items)));
        }
        scope.close();
        runtime.drain_cancelled_tasks().unwrap();
        drop((scope, handle, items));
        if boundary == Boundary::ConversionFault {
            assert_eq!(
                runtime.collect_garbage().unwrap_err().kind(),
                RuntimeErrorKind::EngineFault
            );
        } else {
            runtime.collect_garbage().unwrap();
            assert_eq!(runtime.gc().allocated_objects(), 0);
        }
        assert_eq!(runtime.gc().active_roots(), 0);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
    }
}
