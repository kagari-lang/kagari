//! External native-wait consumer, including builds without source/native features.
use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine, builder::KagariEngineBuilder},
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
    runtime::Runtime,
};
use kagari_runtime::{
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        completion::CompletionStatus,
        conversion::{FromKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};
use kagari_vm::vm::owned::DriveResult;
use std::{
    future::Future,
    pin::Pin,
    sync::mpsc::{self, SyncSender, TrySendError},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
    time::Duration,
};

// Same source-free evidence authoring owner as the embedding unit fixtures.
#[allow(dead_code)] // This consumer needs the builder, not the native-only fixture.
#[path = "../../kagari-runtime/tests/support/native/program.rs"]
mod native_program;

#[path = "support/async_fixture.rs"]
mod fixture;
use fixture::{Fixture, slice};

impl Fixture {
    fn cold(&self, input: i32) -> RootedValue {
        let execution = self
            .runtime
            .backend()
            .start(
                &self.module,
                "create",
                &[Value::I32(input)],
                Default::default(),
            )
            .unwrap();
        let DriveResult::Complete(result) =
            self.runtime.backend().drive(&execution, slice()).unwrap()
        else {
            panic!("cold creation");
        };
        result.unwrap()
    }
}

#[derive(Default)]
struct Notice(AtomicUsize);

impl Wake for Notice {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct RuntimeReady(SyncSender<()>);

impl Wake for RuntimeReady {
    fn wake(self: Arc<Self>) {
        match self.0.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => panic!("host loop disconnected"),
        }
    }
}

struct ActiveI32(i32);

impl KagariType for ActiveI32 {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl FromKagari for ActiveI32 {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        ty: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        assert!(cx.runtime().execution_root().is_some());
        Ok(Self(i32::from_kagari(cx, ty, value)?))
    }
}

#[test]
fn sdk_scheduler_source_free_wake_contract() {
    let mut f = Fixture::new();
    let (send, receive) = mpsc::sync_channel(1);
    let wake = Waker::from(Arc::new(RuntimeReady(send)));
    assert!(f.runtime.poll(&mut Context::from_waker(&wake)).is_pending());
    let create = f
        .runtime
        .async_function::<(i32,), ActiveI32>(&f.module, "create")
        .unwrap();
    for (input, expected) in [(1, 8), (21, 42)] {
        let mut ticket = f.runtime.submit_async(&create, (input,)).unwrap();
        let mut completed = false;
        for _ in 0..32 {
            if let Poll::Ready(value) =
                Pin::new(&mut ticket).poll(&mut Context::from_waker(Waker::noop()))
            {
                assert_eq!(value.unwrap().0, expected);
                completed = true;
                break;
            }
            receive
                .recv_timeout(Duration::from_secs(2))
                .expect("source-free runtime wake");
            assert!(f.runtime.poll(&mut Context::from_waker(&wake)).is_pending());
            if let Some(completion) = f.sent.lock().unwrap().pop() {
                thread::spawn(move || {
                    assert_eq!(completion.complete(Ok(42)), CompletionStatus::Accepted);
                })
                .join()
                .unwrap();
            }
        }
        assert!(completed, "bounded notification-driven completion");
    }
    f.runtime.close().unwrap();
    assert!(matches!(
        f.runtime.poll(&mut Context::from_waker(&wake)),
        Poll::Ready(Ok(()))
    ));
}

#[test]
fn sdk_owned_native_wait_contract() {
    let f = Fixture::new();
    assert!(f.program.bytecode().modules().len() > 1);
    assert!(
        f.runtime
            .execute(&f.module, "run", &[], &Default::default())
            .is_err()
    );
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "synchronous entry cannot submit work"
    );
    let future = f.cold(21);
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    let execution = f
        .runtime
        .backend()
        .start(
            &f.module,
            "wait",
            &[future.value(f.runtime.runtime().gc()).unwrap()],
            Default::default(),
        )
        .unwrap();
    let notice = Arc::new(Notice::default());
    execution.set_waker(&Waker::from(notice.clone()));
    assert_eq!(
        notice.0.load(Ordering::SeqCst),
        1,
        "registration observes queued readiness"
    );
    assert!(matches!(
        f.runtime.backend().drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert!(!execution.is_ready());
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    assert_eq!(*f.inputs.lock().unwrap(), [21]);
    drop(future);
    f.runtime.runtime().collect_garbage().unwrap();
    let completion = f.sent.lock().unwrap().pop().unwrap();
    std::thread::spawn(move || {
        assert_eq!(completion.complete(Ok(42)), CompletionStatus::Accepted);
        assert_eq!(completion.complete(Ok(99)), CompletionStatus::Duplicate);
    })
    .join()
    .unwrap();
    assert_eq!(notice.0.load(Ordering::SeqCst), 2);
    assert!(execution.is_ready());
    drop(f.cold(30));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "readiness and another root never drive this continuation"
    );
    let DriveResult::Complete(result) = f.runtime.backend().drive(&execution, slice()).unwrap()
    else {
        panic!("ready continuation");
    };
    assert_eq!(
        result.unwrap().value(f.runtime.runtime().gc()),
        Some(Value::I32(42))
    );
    assert!(!execution.is_ready());
    assert!(
        f.runtime.backend().drive(&execution, slice()).is_err(),
        "retired owner cannot resume twice"
    );
    for (input, output) in [(0, 7), (1, 8)] {
        let future = f.cold(input);
        let execution = f
            .runtime
            .backend()
            .start_future(
                &future.value(f.runtime.runtime().gc()).unwrap(),
                Default::default(),
            )
            .unwrap();
        let DriveResult::Complete(result) = f.runtime.backend().drive(&execution, slice()).unwrap()
        else {
            panic!("immediate publication");
        };
        assert_eq!(
            result.unwrap().value(f.runtime.runtime().gc()),
            Some(Value::I32(output))
        );
    }
    let shared = ExecutionContext::default();
    let left_future = f.cold(15);
    let right_future = f.cold(16);
    let left = f
        .runtime
        .backend()
        .start(
            &f.module,
            "wait",
            &[left_future.value(f.runtime.runtime().gc()).unwrap()],
            shared.interpreter_options().unwrap(),
        )
        .unwrap();
    let right = f
        .runtime
        .backend()
        .start(
            &f.module,
            "wait",
            &[right_future.value(f.runtime.runtime().gc()).unwrap()],
            shared.interpreter_options().unwrap(),
        )
        .unwrap();
    for owner in [&left, &right] {
        assert!(matches!(
            f.runtime.backend().drive(owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
    }
    let right_completion = f.sent.lock().unwrap().pop().unwrap();
    let left_completion = f.sent.lock().unwrap().pop().unwrap();
    left.cancel();
    assert!(
        shared.cancellation.check().is_ok(),
        "owner cancellation must not cancel the shared host context"
    );
    assert!(!right.is_ready(), "independent waiter remains pending");
    assert!(matches!(
        f.runtime.backend().drive(&left, slice()).unwrap(),
        DriveResult::Complete(Err(error)) if matches!(EmbeddingError::from(error.clone()), EmbeddingError::Runtime {
            kind: RuntimeFailureKind::Cancelled,
            ..
        })
    ));
    assert_eq!(left_completion.complete(Ok(99)), CompletionStatus::Stale);
    assert_eq!(
        right_completion.complete(Ok(43)),
        CompletionStatus::Accepted
    );
    let DriveResult::Complete(result) = f.runtime.backend().drive(&right, slice()).unwrap() else {
        panic!("uncancelled sibling execution");
    };
    assert_eq!(
        result.unwrap().value(f.runtime.runtime().gc()),
        Some(Value::I32(43))
    );
    for abandon in [false, true] {
        let future = f.cold(10);
        let context = ExecutionContext::default();
        let execution = f
            .runtime
            .backend()
            .start_future(
                &future.value(f.runtime.runtime().gc()).unwrap(),
                context.interpreter_options().unwrap(),
            )
            .unwrap();
        assert!(matches!(
            f.runtime.backend().drive(&execution, slice()).unwrap(),
            DriveResult::Waiting
        ));
        let completion = f.sent.lock().unwrap().pop().unwrap();
        if abandon {
            drop(execution);
            assert_eq!(f.runtime.runtime().drain_retired_executions().unwrap(), 1);
        } else {
            let wake = Arc::new(Notice::default());
            execution.set_waker(&Waker::from(wake.clone()));
            context.cancellation.cancel();
            assert_eq!(wake.0.load(Ordering::SeqCst), 1);
            let DriveResult::Complete(Err(error)) =
                f.runtime.backend().drive(&execution, slice()).unwrap()
            else {
                panic!("terminal cancellation");
            };
            let EmbeddingError::Runtime { kind, .. } = EmbeddingError::from(error) else {
                panic!("runtime cancellation diagnostic");
            };
            assert_eq!(kind, RuntimeFailureKind::Cancelled);
        }
        assert_eq!(completion.complete(Ok(9)), CompletionStatus::Stale);
    }
    assert_eq!(f.cancels.load(Ordering::SeqCst), 3);
    assert_eq!(
        f.runtime
            .runtime()
            .resources()
            .counters()
            .current_call_depth,
        0
    );
}

#[cfg(feature = "native")]
mod native {
    use super::{Fixture, slice};
    use kagari_abi::native::{BackendId, BackendTarget};
    use kagari_codegen::{
        BackendConfiguration, BackendFunctionInput, CodegenBackend, diagnostic::BackendCompileError,
    };
    use kagari_contract::native::NativeCompilationProduct;
    use kagari_runtime::{native::completion::CompletionStatus, value::Value};
    use kagari_vm::vm::native::PreparedNativeEntry;
    use kagari_vm::vm::owned::DriveResult;
    use std::sync::atomic::Ordering;

    struct Backend {
        calls: usize,
    }

    // SAFETY: this backend only reports unsupported input and never publishes code.
    unsafe impl CodegenBackend for Backend {
        fn configuration(&self) -> BackendConfiguration {
            BackendConfiguration {
                backend: BackendId::new("async-decline"),
                target: BackendTarget::new("host", usize::BITS as u8),
                options: vec![],
            }
        }

        fn compile_function(
            &mut self,
            _: BackendFunctionInput<'_>,
        ) -> Result<NativeCompilationProduct, BackendCompileError> {
            self.calls += 1;
            Err(BackendCompileError::unsupported(
                "fixture must not compile a resume body",
            ))
        }
    }

    #[test]
    fn sdk_native_preparation_declines_resume_before_entry() {
        let f = Fixture::new();
        let mut backend = Backend { calls: 0 };
        let prepared = f
            .runtime
            .prepare_native(
                &f.program,
                &f.module,
                "run",
                &mut backend,
                &Default::default(),
            )
            .unwrap();
        let PreparedNativeEntry::Unsupported { diagnostics, .. } = &prepared else {
            panic!("resume body requires interpreter");
        };
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains("owned interpreter driver"))
        );
        assert_eq!(backend.calls, 0);
        assert!(
            f.runtime
                .execute_prepared(&f.module, "run", &[], &Default::default(), &prepared)
                .is_err()
        );
        assert_eq!(f.starts.load(Ordering::SeqCst), 0);
        let execution = f
            .runtime
            .backend()
            .start(&f.module, "run", &[], Default::default())
            .unwrap();
        assert!(matches!(
            f.runtime.backend().drive(&execution, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(f.starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
            CompletionStatus::Accepted
        );
        let DriveResult::Complete(result) = f.runtime.backend().drive(&execution, slice()).unwrap()
        else {
            panic!("interpreter continuation");
        };
        assert_eq!(
            result.unwrap().value(f.runtime.runtime().gc()),
            Some(Value::I32(42))
        );
        assert_eq!(backend.calls, 0);
    }
}
