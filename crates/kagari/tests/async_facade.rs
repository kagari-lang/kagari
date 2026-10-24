#![cfg(feature = "source")]
//! Public typed calls: no raw values, factory staging or per-Task driving.
use kagari::{
    context::ExecutionContext,
    engine::KagariEngine,
    error::{EmbeddingError, RuntimeFailureKind},
    runtime::Runtime,
};
use kagari_runtime::{
    frame::types::arguments::TypeArgument,
    gc::roots::RootSet,
    module::Program,
    native::{
        adapter::Async,
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        completion::{Completion, CompletionStatus},
        conversion::{
            FromKagari, IntoKagari, KagariType,
            arguments::{IntoKagariArguments, KagariArguments},
            context::ConversionContext,
        },
        declarations::FunctionDecl,
        future::NativeStart,
        interfaces::Interface,
        typed::NativeContext,
        types::Type,
    },
    task::value::TaskScope,
    value::Value,
};
use kagari_source::source::SourceFile;
use std::{
    cell::Cell,
    future::{Future, poll_fn},
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
    time::Duration,
};

// Execute the documented public workflow rather than maintaining a second copy.
#[path = "../examples/host_objects.rs"]
mod objects_example;

#[test]
fn relative_typed_members_and_current_reload_workflow() {
    objects_example::main();
}

const SOURCE: &str = include_str!("fixtures/embedding/async/public-facade.kgr");

struct Fixture {
    runtime: Runtime,
    module: Program,
    starts: Arc<AtomicUsize>,
    touches: Arc<AtomicUsize>,
    cancels: Arc<AtomicUsize>,
    sent: Arc<Mutex<Vec<Completion<i32>>>>,
}

impl Fixture {
    fn new() -> Self {
        let starts = Arc::new(AtomicUsize::new(0));
        let touches = Arc::new(AtomicUsize::new(0));
        let cancels = Arc::new(AtomicUsize::new(0));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut engine = KagariEngine::builder().unwrap();
        let mut provider = ModuleBuilder::new("test::typed", engine.declarations()).unwrap();
        let touch = provider
            .define_function(
                FunctionDecl::new("touch")
                    .parameter("value", Type::i32())
                    .returns(Type::unit()),
            )
            .unwrap();
        let called = touches.clone();
        provider
            .bind(
                touch,
                move |_: &mut NativeContext<'_>, (_value,): (i32,)| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .unwrap();
        let future = provider
            .declarations()
            .future_type()
            .unwrap()
            .apply([Type::i32()])
            .unwrap();
        let request = provider
            .define_function(
                FunctionDecl::new("request")
                    .parameter("value", Type::i32())
                    .returns(future),
            )
            .unwrap();
        let (started, cancelled, send) = (starts.clone(), cancels.clone(), sent.clone());
        provider
            .bind::<Async<(i32,)>, i32>(request, move |(value,): (i32,), done: Completion<i32>| {
                started.fetch_add(1, Ordering::SeqCst);
                if value == 42 {
                    thread::spawn(move || {
                        done.complete(Ok(84));
                    });
                    return Ok(NativeStart::pending());
                }
                send.lock().unwrap().push(done);
                let cancelled = cancelled.clone();
                Ok(NativeStart::cancellable(move || {
                    cancelled.fetch_add(1, Ordering::SeqCst);
                }))
            })
            .unwrap();
        engine.install(provider.finish().unwrap()).unwrap();
        let engine = engine.build().unwrap();
        let program = engine
            .compile_to_program(SourceFile::new("typed.kgr", SOURCE), Default::default())
            .unwrap();
        let mut context = ExecutionContext::default();
        context.inputs.unix_time_millis = 456;
        let mut runtime = engine.runtime(context);
        let module = runtime.load_program(&program, Default::default()).unwrap();
        Self {
            runtime,
            module,
            starts,
            touches,
            cancels,
            sent,
        }
    }
}

struct Notice(SyncSender<()>);

impl Wake for Notice {
    fn wake(self: Arc<Self>) {
        match self.0.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => panic!("disconnected host adapter"),
        }
    }
}

fn notice() -> (Waker, Receiver<()>) {
    let (send, receive) = mpsc::sync_channel(1);
    (Waker::from(Arc::new(Notice(send))), receive)
}

fn cancelled<T>(result: Result<T, EmbeddingError>) {
    assert!(matches!(
        result,
        Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::Cancelled,
            ..
        })
    ));
}

#[derive(Debug)]
struct Output {
    value: Cell<i32>,
    time: i64,
}

impl KagariType for Output {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl FromKagari for Output {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        ty: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        Ok(Self {
            value: Cell::new(i32::from_kagari(cx, ty, value)?),
            time: cx.runtime().execution_time_millis()?,
        })
    }
}

struct Input(Rc<i32>);

impl KagariType for Input {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl IntoKagari for Input {
    fn into_kagari(self, cx: &mut ConversionContext<'_>, ty: &TypeArgument) -> NativeResult<Value> {
        assert_eq!(cx.runtime().execution_time_millis()?, 123);
        (*self.0).into_kagari(cx, ty)
    }
}

struct PanickingInput;

impl KagariType for PanickingInput {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl IntoKagari for PanickingInput {
    fn into_kagari(self, _: &mut ConversionContext<'_>, _: &TypeArgument) -> NativeResult<Value> {
        panic!("argument converter panic");
    }
}

struct PanickingArguments;

impl KagariArguments for PanickingArguments {
    fn argument_types(catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
        <(i32,)>::argument_types(catalog)
    }
}

impl IntoKagariArguments for PanickingArguments {
    fn into_arguments(
        self,
        _: &mut ConversionContext<'_>,
        _: &[TypeArgument],
    ) -> NativeResult<RootSet> {
        panic!("argument converter panic");
    }
}

#[test]
fn panicking_async_admission_retires_without_another_driver_turn() {
    let mut f = Fixture::new();
    let function = f
        .runtime
        .async_function::<PanickingArguments, i32>(&f.module.root(), "factory")
        .unwrap();
    let make = f
        .runtime
        .function::<(), Interface>(&f.module.root(), "loader")
        .unwrap();
    let loader = f.runtime.call(&make, ()).unwrap();
    let method = f
        .runtime
        .async_interface_method::<(PanickingInput,), i32>(&loader, "load")
        .unwrap();
    let roots = f.runtime.runtime().gc().active_roots();
    let mut retired = Vec::new();
    for interface in [false, true] {
        let panic = catch_unwind(AssertUnwindSafe(|| {
            if interface {
                let _ =
                    f.runtime
                        .submit_async_interface_method(&loader, &method, (PanickingInput,));
            } else {
                let _ = f.runtime.submit_async(&function, PanickingArguments);
            }
        }))
        .expect_err("the original converter panic must propagate");
        assert_eq!(
            panic.downcast_ref::<&str>(),
            Some(&"argument converter panic")
        );
        assert!(f.runtime.runtime().execution_root().is_none());
        assert_eq!(f.runtime.runtime().gc().active_roots(), roots);
        // This must be an empty observation, not the operation that performs cleanup.
        retired.push(f.runtime.runtime().drain_retired_executions().unwrap());
    }
    assert_eq!(
        retired,
        [0, 0],
        "neither admission may leave an abandoned owner"
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    assert_eq!(f.touches.load(Ordering::SeqCst), 0);
    assert!(f.runtime.runtime().check_scheduler_health().is_ok());
}

#[test]
fn typed_direct_await_drives_tasks_without_a_pump_and_preserves_context() {
    let mut f = Fixture::new();
    let function = f
        .runtime
        .async_function::<(Input,), Output>(&f.module.root(), "factory")
        .unwrap();
    let mut context = ExecutionContext::default();
    context.inputs.unix_time_millis = 123;
    let call = f
        .runtime
        .call_async_with_context(&function, (Input(Rc::new(42)),), &context)
        .unwrap();
    assert_eq!(
        f.touches.load(Ordering::SeqCst),
        0,
        "admission does not run factory"
    );
    fn send<T: Send>(_: &T) {}
    send(&call);
    let executor = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let output = executor.block_on(call).unwrap();
    assert_eq!(output.value.get(), 84);
    assert_eq!(output.time, 123);
    assert_eq!(f.touches.load(Ordering::SeqCst), 1);
    let scope = f.runtime.create_task_scope(&f.module).unwrap();
    let scoped = f
        .runtime
        .async_function::<(TaskScope,), i32>(&f.module.root(), "scoped")
        .unwrap();
    assert_eq!(
        executor
            .block_on(f.runtime.call_async(&scoped, (scope.handle(),)).unwrap())
            .unwrap(),
        84
    );
    assert!(f.runtime.take_task_event().is_none());
}

#[test]
fn typed_call_drop_cleans_up_without_another_poll() {
    let mut f = Fixture::new();
    let function = f
        .runtime
        .async_function::<(i32,), i32>(&f.module.root(), "factory")
        .unwrap();
    drop(f.runtime.call_async(&function, (10,)).unwrap());
    assert_eq!(f.touches.load(Ordering::SeqCst), 0);
    let mut call = f.runtime.call_async(&function, (10,)).unwrap();
    assert!(
        Pin::new(&mut call)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    let late = f.sent.lock().unwrap().pop().unwrap();
    drop(call);
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(late.complete(Ok(99)), CompletionStatus::Stale);

    let pair = f
        .runtime
        .async_function::<(), (i32, i32)>(&f.module.root(), "pair")
        .unwrap();
    let mut call = f.runtime.call_async(&pair, ()).unwrap();
    for _ in 0..100 {
        assert!(
            Pin::new(&mut call)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        if f.starts.load(Ordering::SeqCst) == 3 {
            break;
        }
    }
    assert_eq!(f.starts.load(Ordering::SeqCst), 3);
    drop(call);
    assert_eq!(
        f.cancels.load(Ordering::SeqCst),
        3,
        "Call drop synchronously retires both branches"
    );
    for late in f.sent.lock().unwrap().drain(..) {
        assert_eq!(late.complete(Ok(99)), CompletionStatus::Stale);
    }
}

#[test]
fn typed_pending_lifetime_contract() {
    let mut f = Fixture::new();
    let factory = f
        .runtime
        .async_function::<(i32,), Output>(&f.module.root(), "factory")
        .unwrap();
    drop(f.runtime.submit_async(&factory, (10,)).unwrap());
    assert!(
        f.runtime
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert_eq!(
        f.touches.load(Ordering::SeqCst),
        0,
        "ticket drop cancels before factory entry"
    );
    let pending = f.runtime.submit_async(&factory, (42,)).unwrap();
    let executor = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    executor.block_on(poll_fn(|cx| {
        assert!(f.runtime.poll(cx).is_pending());
        if pending.is_finished() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }));
    drop(f.runtime);
    let output = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(pending)
            .unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(output.value.get(), 84);
    assert_eq!(
        output.time, 456,
        "default context retained through output conversion"
    );
}

#[test]
fn typed_submission_teardown_and_external_driver_handoff() {
    let mut f = Fixture::new();
    let (external, ready) = notice();
    assert!(
        f.runtime
            .poll(&mut Context::from_waker(&external))
            .is_pending()
    );
    let scope = f.runtime.create_task_scope(&f.module).unwrap();
    let launch = f
        .runtime
        .function::<(TaskScope,), ()>(&f.module.root(), "launch")
        .unwrap();
    f.runtime.call(&launch, (scope.handle(),)).unwrap();
    let load = f
        .runtime
        .async_function::<(i32,), i32>(&f.module.root(), "load")
        .unwrap();
    let mut submitted = f.runtime.submit_async(&load, (11,)).unwrap();
    let long = f
        .runtime
        .async_function::<(), i32>(&f.module.root(), "long")
        .unwrap();
    let executor = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert_eq!(
        executor
            .block_on(f.runtime.call_async(&long, ()).unwrap())
            .unwrap(),
        500
    );
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        2,
        "Call drives independent scope and submitted work"
    );
    let mut dropped = f.runtime.call_async(&load, (12,)).unwrap();
    assert!(
        Pin::new(&mut dropped)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    let abandoned = f.sent.lock().unwrap().pop().unwrap();
    drop(dropped);
    assert_eq!(abandoned.complete(Ok(0)), CompletionStatus::Stale);
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    while ready.try_recv().is_ok() {
        assert!(
            f.runtime
                .poll(&mut Context::from_waker(&external))
                .is_pending()
        );
    }
    let completions = f.sent.lock().unwrap().drain(..).collect::<Vec<_>>();
    thread::spawn(move || {
        for done in completions {
            done.complete(Ok(9));
        }
    })
    .join()
    .unwrap();
    for _ in 0..100 {
        if submitted.is_finished() {
            break;
        }
        ready
            .recv_timeout(Duration::from_secs(2))
            .expect("external route restored");
        assert!(
            f.runtime
                .poll(&mut Context::from_waker(&external))
                .is_pending()
        );
    }
    assert!(matches!(
        Pin::new(&mut submitted).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(9))
    ));
    let mut unfinished = f.runtime.submit_async(&load, (10,)).unwrap();
    assert!(
        f.runtime
            .poll(&mut Context::from_waker(&external))
            .is_pending()
    );
    let late = f.sent.lock().unwrap().pop().unwrap();
    drop(f.runtime);
    cancelled(executor.block_on(&mut unfinished));
    assert_eq!(late.complete(Ok(9)), CompletionStatus::Stale);
}
