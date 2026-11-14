//! Actual Native -> script -> Native recursion and root reuse after exhaustion.
use crate::{
    Runtime, RuntimeConfig,
    error::reason::ErrorReason,
    interpreter_tests::common::{compile_with_provider, round_trip_bytecode},
    module::{ExecutableModule, InstalledProgram, entry::ProgramEntry},
    native::{
        binding::NativeResult, builder::ModuleBuilder, callable::CallableHandle,
        context::NativeCall, declarations::FunctionDecl, types::Type,
    },
    resource::RuntimeLimits,
    test_support::bytecode::load_bytecode,
};
#[cfg(all(not(target_family = "wasm"), not(miri)))]
use std::thread;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Fixture {
    runtime: Runtime,
    _program: InstalledProgram,
    owner: ExecutableModule,
    calls: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl Fixture {
    fn new(depth: usize) -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut module = ModuleBuilder::new("test::stack", &Default::default()).unwrap();
        let choose = module
            .define_function(
                FunctionDecl::new("choose")
                    .parameter("run", Type::bool())
                    .parameter("callback", Type::function([], Type::i32()).unwrap())
                    .returns(Type::i32()),
            )
            .unwrap();
        let observed = calls.clone();
        let observed_peak = peak.clone();
        module
            .bind(
                choose,
                move |cx: &mut NativeCall<'_>,
                      run: bool,
                      callback: CallableHandle<'_>|
                      -> NativeResult<i32> {
                    observed.fetch_add(1, Ordering::Relaxed);
                    observed_peak
                        .fetch_max(cx.runtime.resources.native_depth.get(), Ordering::Relaxed);
                    if run { callback.call(cx, ()) } else { Ok(7) }
                },
            )
            .unwrap();
        let module = module.finish().unwrap();
        let code = round_trip_bytecode(compile_with_provider(
            include_str!("../../../tests/fixtures/execution/native-stack.kgr"),
            Some(&module),
        ));
        let mut runtime = Runtime::new(RuntimeConfig {
            limits: RuntimeLimits {
                max_native_depth: NonZeroUsize::new(depth).unwrap(),
                ..Default::default()
            },
            ..Default::default()
        });
        module.install(&mut runtime).unwrap();
        let program = load_bytecode(&mut runtime, "native-stack", code).unwrap();
        let owner = runtime.select_program(&program).unwrap();
        runtime.collect_garbage().unwrap();
        Self {
            runtime,
            _program: program,
            owner,
            calls,
            peak,
        }
    }

    fn run(mut self) -> usize {
        let Self {
            runtime,
            owner,
            calls,
            peak,
            ..
        } = &mut self;
        let baseline = runtime.gc_stats();
        let error = runtime
            .execute_module(
                owner,
                ProgramEntry::Export("exhaust"),
                runtime.execution_options(),
            )
            .unwrap_err();
        assert_eq!(error.reason(), &ErrorReason::NativeStackOverflow);
        assert!(
            error
                .trace()
                .unwrap()
                .frames
                .iter()
                .any(|frame| frame.function_name == "choose")
        );
        assert!(calls.load(Ordering::Relaxed) > 0);
        assert_eq!(runtime.resources.native_depth.get(), 0);
        assert_eq!(runtime.resources.counters().current_call_depth, 0);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(
            runtime
                .execute_module(
                    owner,
                    ProgramEntry::Export("healthy"),
                    runtime.execution_options()
                )
                .unwrap()
                .return_value
                .as_i32(),
            Some(7)
        );
        assert_eq!(runtime.resources.native_depth.get(), 0);
        assert!(!runtime.is_quarantined());
        peak.load(Ordering::Relaxed)
    }
}

#[test]
fn native_guard_bounds_reentry_and_releases_on_failure() {
    assert_eq!(Fixture::new(4).run(), 4);
}

#[test]
#[cfg(all(not(target_family = "wasm"), not(miri)))]
fn native_guard_reserves_remaining_os_stack_before_its_depth_ceiling() {
    // Compile before entering the deliberately small stack; compilation itself
    // is outside Runtime's execution-stack contract.
    let fixture = Fixture::new(1024);
    let peak = thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            stacker::remaining_stack()?;
            Some(fixture.run())
        })
        .unwrap()
        .join()
        .unwrap();
    if let Some(peak) = peak {
        println!("NATIVE_STACK_GUARD,thread_stack_bytes=262144,depth_limit=1024,peak={peak}");
        assert!(peak < 1024);
    }
}
