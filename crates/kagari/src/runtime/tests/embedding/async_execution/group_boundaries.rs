//! Fault, context and generation boundaries distinct from pair scheduling policy.
use super::{
    Fixture,
    scheduler::{HostLoop, decode, result, scheduled},
};
use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::EngineConfig,
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
};
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    frame::ExecutionFrame,
    host::HostCallContext,
    module::LoadedModule,
    native::{
        adapter::Async,
        binding::NativeResult,
        builder::ModuleBuilder,
        completion::{Completion, CompletionStatus},
        declarations::FunctionDecl,
        future::NativeStart,
        types::Type,
    },
    scheduler::tasks::TaskEvent,
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::declaration::host::HostCallPolicy;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

#[derive(Debug)]
struct FatalBranchEntry(Arc<Mutex<Option<Arc<ErrorTrace>>>>);

impl ExecutionObserver for FatalBranchEntry {
    fn begin(&mut self, runtime: &Runtime, _: &LoadedModule) -> NativeResult<()> {
        let trace = runtime.capture_error_trace();
        *self.0.lock().unwrap() = Some(trace.clone());
        Err(RuntimeError::new(
            RuntimeErrorKind::EngineFault,
            "recognizable branch entry fault",
        )
        .with_trace(trace))
    }

    fn observe(
        &mut self,
        _: &Runtime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> NativeResult<()> {
        Ok(())
    }
}

#[test]
fn groups_fatal_branch_entry_preserves_selected_failure() {
    for (nested, task_root) in [(false, false), (true, false), (true, true)] {
        let mut f = Fixture::source(
            r#"
use core::task::join;
use test::async_sdk::request;
async fn right() -> i32 { request(20).await }
async fn pair() -> i32 { val (a,b)=join(request(10),right()).await; a+b }
pub async fn direct() -> i32 { pair().await }
pub async fn nested() -> i32 { val (a,b)=join(pair(),async { 1 }).await; a+b }
pub fn factory() -> fn()->Future<i32> { || nested() }
"#,
        );
        let host = HostLoop::new();
        assert!(f.runtime.poll(&mut host.context()).is_pending());
        let scope = task_root.then(|| {
            f.runtime
                .create_scheduled_task_scope(&f.module, &Default::default())
                .unwrap()
        });
        let mut ticket = if let Some(scope) = &scope {
            let factory = f.invoke("factory", &[]);
            drop(
                f.runtime
                    .runtime()
                    .spawn_task(&f.value(scope.capability()), &f.value(&factory))
                    .unwrap()
                    .unwrap(),
            );
            None
        } else {
            Some(scheduled(&f, if nested { "nested" } else { "direct" }, &[]))
        };
        host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 2);
        host.settle(&mut f.runtime);
        let trace = Arc::new(Mutex::new(None));
        f.runtime
            .runtime()
            .set_execution_observer(FatalBranchEntry(trace.clone()))
            .unwrap();
        let right = f.sent.lock().unwrap().pop().unwrap();
        assert_eq!(right.complete(Ok(2)), CompletionStatus::Accepted);
        let Poll::Ready(Err(EmbeddingError::Runtime {
            kind,
            message,
            trace: terminal_trace,
            ..
        })) = f.runtime.poll(&mut host.context())
        else {
            panic!("fatal branch must terminate the scheduler");
        };
        assert_eq!(kind, RuntimeFailureKind::EngineInvariant);
        assert!(f.runtime.runtime().is_quarantined());
        assert_eq!(
            f.cancels.load(Ordering::SeqCst),
            2,
            "waiting sibling and unpolled right consumer cleaned before publication"
        );
        let late = f.sent.lock().unwrap().pop().unwrap();
        assert_eq!(late.complete(Ok(9)), CompletionStatus::Stale);
        let selected = trace.lock().unwrap().clone().unwrap();
        assert!(
            selected
                .frames
                .iter()
                .any(|frame| frame.function_name.contains("right"))
        );
        let failure = if let Some(ticket) = &mut ticket {
            let failure = result(ticket).unwrap_err();
            assert!(Arc::ptr_eq(failure.trace().unwrap(), &selected));
            failure
        } else {
            let Some(TaskEvent::Failed(event)) = f.runtime.take_task_event() else {
                panic!("owning Task must publish its selected failure")
            };
            assert_eq!(event.failure.error.trace().unwrap().frames, selected.frames);
            event.failure.error
        };
        assert_eq!(failure.kind(), RuntimeErrorKind::EngineFault);
        assert_eq!(failure.message(), "recognizable branch entry fault");
        assert_eq!(message, "recognizable branch entry fault");
        assert!(Arc::ptr_eq(terminal_trace.as_ref().unwrap(), &selected));
        drop(scope);
        f.runtime.runtime().drain_cancelled_tasks().unwrap();
        assert_eq!(f.runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn groups_cleanup_fault_overrides_err_and_drains_nested_siblings() {
    for fatal_entry in [false, true] {
        group_cleanup_fault_case(fatal_entry);
    }
}

fn group_cleanup_fault_case(fatal_entry: bool) {
    let starts = Arc::new(AtomicUsize::new(0));
    let cancels = Arc::new(AtomicUsize::new(0));
    let completions = Arc::new(Mutex::new(Vec::new()));
    let mut f = Fixture::with_provider(Default::default(), |engine| {
        let mut module = ModuleBuilder::new("test::fault", engine.declarations()).unwrap();
        let future = module
            .declarations()
            .future_type()
            .unwrap()
            .apply([Type::i32()])
            .unwrap();
        let call = module
            .define_function(
                FunctionDecl::new("wait")
                    .parameter("input", Type::i32())
                    .returns(future),
            )
            .unwrap();
        let (starts, cancels, completions) = (starts.clone(), cancels.clone(), completions.clone());
        module
            .bind::<Async<(i32,)>, i32>(call, move |(input,): (i32,), done: Completion<i32>| {
                starts.fetch_add(1, Ordering::SeqCst);
                completions.lock().unwrap().push(done);
                let cancels = cancels.clone();
                Ok(NativeStart::cancellable(move || {
                    cancels.fetch_add(1, Ordering::SeqCst);
                    assert_ne!(input, 10, "host cancellation cleanup fault");
                }))
            })
            .unwrap();
        engine.install(module.finish().unwrap()).unwrap();
    });
    f.load_source(
        r#"
use core::task::{join,try_join};
use test::fault::wait;
use test::async_sdk::request;
pub async fn run() -> Result<(i32,i32),String> {
    try_join(async { val (a,b)=join(wait(10),wait(20)).await; Ok(a+b) },
        async { request(30).await; Err("selected") }).await
}
"#,
    );
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let future = f.invoke("run", &[]);
    let mut ticket = f
        .runtime
        .runtime()
        .schedule_execution(f.start_future(&future), |_, _| -> Result<(), _> {
            panic!("fault must precede publication")
        })
        .unwrap();
    host.until(&mut f.runtime, || {
        starts.load(Ordering::SeqCst) == 2 && f.starts.load(Ordering::SeqCst) == 1
    });
    host.settle(&mut f.runtime);
    let trace = Arc::new(Mutex::new(None));
    if fatal_entry {
        f.runtime
            .runtime()
            .set_execution_observer(FatalBranchEntry(trace.clone()))
            .unwrap();
    }
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(1));
    let Poll::Ready(Err(EmbeddingError::Runtime {
        message,
        trace: terminal_trace,
        ..
    })) = f.runtime.poll(&mut host.context())
    else {
        panic!("cleanup fault must terminate the scheduler");
    };
    assert!(f.runtime.runtime().is_quarantined());
    let error = result(&mut ticket).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert_eq!(error.message(), "owned execution cleanup failed");
    assert_eq!(message, "owned execution cleanup failed");
    if fatal_entry {
        let original = trace.lock().unwrap().clone().unwrap();
        assert!(!original.frames.is_empty());
        assert!(Arc::ptr_eq(error.trace().unwrap(), &original));
        assert!(Arc::ptr_eq(terminal_trace.as_ref().unwrap(), &original));
    }
    assert_eq!(
        cancels.load(Ordering::SeqCst),
        2,
        "faulting cleanup does not skip the sibling"
    );
    for late in completions.lock().unwrap().drain(..) {
        assert_eq!(late.complete(Ok(9)), CompletionStatus::Stale);
    }
    assert_eq!(
        f.runtime.runtime().gc().active_roots(),
        1,
        "only retained outer Future remains"
    );
}

#[test]
fn groups_child_trap_and_sticky_parent_cancel_close_waits() {
    let mut f = Fixture::source(
        r#"
use core::task::join;
use test::async_sdk::request;
pub async fn trap() -> i32 { val (a,b)=join(request(10),async { val zero=0; 1/zero }).await; a+b }
pub async fn waiting() -> i32 { val (a,b)=join(request(10),request(20)).await; a+b }
"#,
    );
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut ticket = scheduled(&f, "trap", &[]);
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(
        result(&mut ticket).unwrap_err().kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(2)),
        CompletionStatus::Stale
    );
    let context = ExecutionContext::default();
    let future = f.invoke("waiting", &[]);
    let execution = f.runtime.start_future(&f.value(&future), &context).unwrap();
    let mut ticket = f
        .runtime
        .runtime()
        .schedule_execution(execution, decode)
        .unwrap();
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 3);
    host.settle(&mut f.runtime);
    for complete in f.sent.lock().unwrap().drain(..) {
        complete.complete(Ok(3));
    }
    context.cancellation.cancel();
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(
        result(&mut ticket).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled,
        "parent termination wins over already-ready branch values"
    );
    host.settle(&mut f.runtime);
}

#[test]
fn groups_share_parent_inputs_rng_and_host_recording() {
    let random = Arc::new(Mutex::new(Vec::new()));
    let mut f = Fixture::with_provider(Default::default(), |engine| {
        let mut module = ModuleBuilder::new("test::context", engine.declarations()).unwrap();
        let function = module
            .define_function(FunctionDecl::new("probe").returns(Type::i32()).host_policy(
                HostCallPolicy {
                    passing: vec![],
                    effects: Default::default(),
                },
            ))
            .unwrap();
        let random = random.clone();
        module
            .bind(function, move |cx: &HostCallContext<'_>, _: &[Value]| {
                assert_eq!(cx.execution_time_millis().unwrap(), 123);
                random
                    .lock()
                    .unwrap()
                    .push(cx.next_execution_random_u64().unwrap());
                Ok(Value::I32(1))
            })
            .unwrap();
        engine.install(module.finish().unwrap()).unwrap();
    });
    f.load_source(r#"
use test::context::probe;
pub async fn grouped() -> i32 { probe(); val (a,b)=core::task::join(async { probe() },async { probe() }).await; probe()+a+b }
pub async fn serial() -> i32 { probe(); val a=probe(); val b=probe(); probe()+a+b }
"#);
    let mut context = ExecutionContext {
        tracing_enabled: true,
        ..Default::default()
    };
    context.inputs.unix_time_millis = 123;
    context.inputs.random_seed = 567;
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    for entry in ["grouped", "serial"] {
        let future = f.invoke(entry, &[]);
        let execution = f.runtime.start_future(&f.value(&future), &context).unwrap();
        let mut ticket = f
            .runtime
            .runtime()
            .schedule_execution(execution, |runtime, value| {
                let owner = runtime.execution_root().unwrap();
                let session = runtime.begin_execution(&owner, runtime.execution_options())?;
                let trace = session.trace().unwrap();
                assert_eq!(
                    trace.host_calls.len(),
                    4,
                    "branch calls record into original invocation"
                );
                assert!(trace.host_calls.iter().all(|call| call.outcome.is_some()));
                decode(runtime, value)
            })
            .unwrap();
        host.until(&mut f.runtime, || ticket.is_finished());
        assert_eq!(result(&mut ticket).unwrap(), 3);
    }
    let values = random.lock().unwrap();
    assert_eq!(values.len(), 8);
    assert_eq!(
        &values[..4],
        &values[4..],
        "siblings share one deterministic stream"
    );
    assert!(values[..4].windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
fn groups_cold_and_waiting_branches_pin_reload_generation() {
    let source = r#"
use test::async_sdk::request;
pub fn version() -> i32 { 1 }
async fn branch() -> i32 { request(10).await+version() }
pub async fn run() -> i32 { val (a,b)=core::task::join(branch(),branch()).await; a+b+version() }
"#;
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut f = Fixture::configured_source(source, config);
    let old = f.module.key();
    let cold = f.invoke("run", &[]);
    let mut waiting = scheduled(&f, "run", &[]);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 2);
    let artifact = f
        .engine
        .compile_to_artifact(
            SourceFile::new("async-script.kgr", source.replace("{ 1 }", "{ 100 }")),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    f.module = f
        .runtime
        .reload_program(&f.module, &program, Default::default())
        .unwrap();
    f.program = program;
    let mut retained = f
        .runtime
        .runtime()
        .schedule_execution(f.start_future(&cold), decode)
        .unwrap();
    drop(cold);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 4);
    f.runtime.runtime().collect_garbage().unwrap();
    assert!(f.runtime.runtime().modules().loaded(old).is_some());
    for done in f.sent.lock().unwrap().drain(..) {
        done.complete(Ok(10));
    }
    host.until(&mut f.runtime, || {
        waiting.is_finished() && retained.is_finished()
    });
    assert_eq!(result(&mut waiting).unwrap(), 23);
    assert_eq!(result(&mut retained).unwrap(), 23);
    f.runtime.runtime().collect_garbage().unwrap();
    assert_eq!(
        f.runtime
            .runtime()
            .modules()
            .retention_counts(old)
            .runtime_values,
        0
    );
    assert_eq!(f.runtime.runtime().gc().active_roots(), 0);
    assert!(
        f.runtime
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
}
