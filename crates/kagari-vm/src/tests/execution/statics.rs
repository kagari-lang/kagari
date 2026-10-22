use crate::{
    error::VmError,
    executor::Executor,
    tests::common::{
        compile_logging_test_bytecode, compile_native_test_bytecode, compile_test_bytecode,
        logging_module, round_trip_bytecode, standard_runtime,
    },
    tests::native_fixtures,
    vm::{JitExecutionStatus, Vm, owned::DriveResult},
};
use kagari_bytecode::instruction::ModuleSlot;
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    module::{LoadedModule, ModuleEpochRetention, entry::ProgramEntry, state::StaticState},
    native::{
        adapter::Async, builder::ModuleBuilder, completion::Completion, declarations::FunctionDecl,
        future::NativeStart, types::Type,
    },
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn call(vm: &Vm, loaded: &LoadedModule, name: &str) -> Value {
    vm.execute(loaded, name)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .unwrap()
}

fn state(runtime: &Runtime, loaded: &LoadedModule, name: &str) -> StaticState {
    let index = loaded
        .bytecode
        .module_slots
        .iter()
        .position(|slot| slot.contract.name == name)
        .unwrap();
    runtime
        .module_instance_snapshot(loaded)
        .unwrap()
        .module_slots[index]
        .clone()
}

#[test]
fn source_and_decoded_statics_are_lazy_shared_on_reload_and_keep_fixed_recipes() {
    let first = compile_test_bytecode(
        r#"
        static var count: i32 = 0;
        static val cold: i32 = 7;
        static val unused: i32 = fail();
        fn fail() -> i32 { val zero = 0; 1 / zero }
        pub fn tick() -> i32 { count += 1; count }
        pub fn read_cold() -> i32 { cold }
        pub fn read_added() -> i32 { 0 }
    "#,
    );
    let second = compile_test_bytecode(
        r#"
        static val added: i32 = 5;
        static val unused: i32 = fail();
        static val cold: i32 = 90;
        static var count: i32 = 100;
        fn fail() -> i32 { val zero = 0; 1 / zero }
        pub fn tick() -> i32 { count += 1; count }
        pub fn read_cold() -> i32 { cold }
        pub fn read_added() -> i32 { added }
    "#,
    );
    for decoded in [false, true] {
        let code = |program| {
            if decoded {
                round_trip_bytecode(program)
            } else {
                program
            }
        };
        let mut runtime = standard_runtime(Default::default());
        let old = runtime.load_program("state", code(first.clone())).unwrap();
        let retained = runtime
            .retain_module(&old, ModuleEpochRetention::CompiledArtifact)
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert!(matches!(
            state(vm.runtime(), &old, "count"),
            StaticState::Uninitialized(_)
        ));
        assert_eq!(call(&vm, &old, "tick"), Value::I32(1));
        let candidate = vm
            .runtime_mut()
            .stage_reload_program(&old, "state", code(second.clone()))
            .unwrap();
        let current = vm.runtime_mut().publish_staged_reload(candidate).unwrap();
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(call(&vm, &current, "tick"), Value::I32(2));
        assert_eq!(call(&vm, &old, "tick"), Value::I32(3));
        assert_eq!(call(&vm, &current, "read_cold"), Value::I32(7));
        assert_eq!(call(&vm, &current, "read_added"), Value::I32(5));
        assert!(matches!(
            state(vm.runtime(), &current, "unused"),
            StaticState::Uninitialized(_)
        ));
        let fresh = vm
            .runtime_mut()
            .load_program("fresh", code(second.clone()))
            .unwrap();
        assert_eq!(call(&vm, &fresh, "tick"), Value::I32(101));
        assert_eq!(call(&vm, &fresh, "read_cold"), Value::I32(90));
        drop(retained);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    }
}

#[test]
fn imported_static_executes_its_fixed_supplier_generation_not_the_callers_module() {
    let source = r#"
        mod provider {
            pub static val value: i32 = helper();
            fn helper() -> i32 { 7 }
        }
        use self::provider::value;
        pub fn read() -> i32 { value }
    "#;
    let first = compile_test_bytecode(source);
    let second = compile_test_bytecode(&source.replace("{ 7 }", "{ 90 }"));
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("imports", round_trip_bytecode(first))
        .unwrap();
    let staged = runtime
        .stage_reload_program(&old, "imports", round_trip_bytecode(second))
        .unwrap();
    let current = runtime.publish_staged_reload(staged).unwrap();
    runtime.collect_garbage().unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(call(&vm, &current, "read"), Value::I32(7));
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    assert_eq!(call(&vm, &current, "read"), Value::I32(7));
}

#[test]
fn initialization_precedes_rhs_and_index_once_with_unused_read_effects_preserved() {
    for (declaration, assignment, result, expected) in [
        (
            "static var target: i32 = mark(1);",
            "target = mark(2);",
            "target",
            122,
        ),
        (
            "static var target: i32 = mark(1);",
            "target += mark(2);",
            "target",
            123,
        ),
        (
            "static val target: [i32] = { mark(1); [0] };",
            "target[index()] = mark(3);",
            "target[0]",
            1233,
        ),
    ] {
        let source = format!(
            r#"
            static var trace: i32 = 0;
            fn mark(x: i32) -> i32 {{ trace = trace * 10 + x; x }}
            fn index() -> u64 {{ mark(2); 0 }}
            {declaration}
            pub fn run() -> i32 {{ {assignment} trace * 10 + {result} }}
        "#
        );
        let program = compile_test_bytecode(&source);
        for decoded in [false, true] {
            let mut runtime = standard_runtime(Default::default());
            let loaded = runtime
                .load_program(
                    "order",
                    if decoded {
                        round_trip_bytecode(program.clone())
                    } else {
                        program.clone()
                    },
                )
                .unwrap();
            let vm = Vm::new(runtime);
            assert_eq!(call(&vm, &loaded, "run"), Value::I32(expected));
        }
    }
}

#[test]
fn static_projection_commit_uses_current_contents_of_the_captured_root() {
    for (binding, index_effect, rhs_effect, assignment, expected) in [
        ("val", "", "BOX.values = [10];", "=", 303),
        ("val", "", "BOX.values = [10];", "+=", 1313),
        ("var", "", "BOX = Box { values: [10] };", "=", 310),
        ("var", "BOX = Box { values: [10] };", "", "=", 310),
        ("val", "BOX.values = [10];", "", "+=", 1313),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
            struct Box {{ var values: [i32] }}
            static {binding} BOX: Box = Box {{ values: [0] }};
            static var trace: i32 = 0;
            fn index() -> u64 {{ trace = trace * 10 + 1; {index_effect} 0 }}
            fn rhs() -> i32 {{ trace = trace * 10 + 2; {rhs_effect} 3 }}
            pub fn run() -> i32 {{
                val original = BOX;
                BOX.values[index()] {assignment} rhs();
                original.values[0] * 100 + BOX.values[0]
            }}
            pub fn effects() -> i32 {{ trace }}
        "#
        ));
        for decoded in [false, true] {
            let mut runtime = standard_runtime(Default::default());
            let loaded = runtime
                .load_program(
                    "projection",
                    if decoded {
                        round_trip_bytecode(program.clone())
                    } else {
                        program.clone()
                    },
                )
                .unwrap();
            let vm = Vm::new(runtime);
            assert_eq!(
                call(&vm, &loaded, "run"),
                Value::I32(expected),
                "{binding}/{index_effect}/{rhs_effect}/{assignment}/decoded={decoded}"
            );
            assert_eq!(call(&vm, &loaded, "effects"), Value::I32(12));
        }
    }
}

#[test]
fn static_projection_bounds_are_checked_after_rhs_with_effects_preserved() {
    for (initial, replacement, succeeds) in [("[]", "[[10]]", true), ("[[0]]", "[]", false)] {
        let program = compile_test_bytecode(&format!(
            r#"
            struct Box {{ var values: [[i32]] }}
            static val BOX: Box = Box {{ values: {initial} }};
            static var trace: i32 = 0;
            fn index() -> u64 {{ trace = trace * 10 + 1; 0 }}
            fn rhs() -> i32 {{ trace = trace * 10 + 2; BOX.values = {replacement}; 3 }}
            pub fn run() -> i32 {{ BOX.values[index()][0] = rhs(); BOX.values[0][0] }}
            pub fn effects() -> i32 {{ trace }}
        "#
        ));
        for decoded in [false, true] {
            let mut runtime = standard_runtime(Default::default());
            let loaded = runtime
                .load_program(
                    "projection_bounds",
                    if decoded {
                        round_trip_bytecode(program.clone())
                    } else {
                        program.clone()
                    },
                )
                .unwrap();
            let vm = Vm::new(runtime);
            if succeeds {
                assert_eq!(call(&vm, &loaded, "run"), Value::I32(3));
            } else {
                let error = vm.execute(&loaded, "run").unwrap_err();
                assert!(
                    matches!(error.cause(), VmError::InvalidIndex(0)),
                    "decoded={decoded}: {error:?}"
                );
            }
            assert_eq!(call(&vm, &loaded, "effects"), Value::I32(12));
        }
    }
}

#[test]
fn cycles_and_traps_poison_only_started_cells_without_replaying_effects() {
    for cyclic in [false, true] {
        let initializer = if cyclic { "through()" } else { "1 / zero()" };
        let program = compile_test_bytecode(&format!(
            r#"
            static var attempts: i32 = 0;
            static val good: i32 = 7;
            static val bad: i32 = initialize();
            static val indirect: i32 = read();
            fn through() -> i32 {{ indirect }}
            fn zero() -> i32 {{ 0 }}
            fn initialize() -> i32 {{ attempts += 1; val saved = good; {initializer} }}
            pub fn read() -> i32 {{ bad }}
            pub fn count() -> i32 {{ attempts }}
        "#
        ));
        for decoded in [false, true] {
            let mut runtime = standard_runtime(Default::default());
            let loaded = runtime
                .load_program(
                    "poison",
                    if decoded {
                        round_trip_bytecode(program.clone())
                    } else {
                        program.clone()
                    },
                )
                .unwrap();
            let mut vm = Vm::new(runtime);
            assert!(vm.execute(&loaded, "read").is_err());
            assert_eq!(call(&vm, &loaded, "count"), Value::I32(1));
            assert!(matches!(
                state(vm.runtime(), &loaded, "good"),
                StaticState::Ready(Value::I32(7))
            ));
            assert!(matches!(
                state(vm.runtime(), &loaded, "bad"),
                StaticState::Failed(_)
            ));
            assert!(if cyclic {
                matches!(
                    state(vm.runtime(), &loaded, "indirect"),
                    StaticState::Failed(_)
                )
            } else {
                matches!(
                    state(vm.runtime(), &loaded, "indirect"),
                    StaticState::Uninitialized(_)
                )
            });
            let staged = vm
                .runtime_mut()
                .stage_reload_program(&loaded, "poison", program.clone())
                .unwrap();
            let next = vm.runtime_mut().publish_staged_reload(staged).unwrap();
            assert!(vm.execute(&next, "read").is_err());
            assert_eq!(call(&vm, &next, "count"), Value::I32(1));
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            assert_eq!(vm.runtime().gc().active_roots(), 0);
        }
    }
}

#[test]
fn native_reentry_cannot_swallow_a_cycle_and_commit_success() {
    let source = r#"
        static val value: i32 = initialize();
        fn initialize() -> i32 { print("reenter"); 42 }
        pub fn read() -> i32 { value }
    "#;
    let code = compile_logging_test_bytecode(source);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut runtime = standard_runtime(Default::default());
    logging_module(move |cx, _| {
        called.fetch_add(1, Ordering::Relaxed);
        cx.runtime().collect_garbage().unwrap();
        let root = cx.runtime().execution_root().unwrap();
        let (owner, function) = root.exported_function("read").unwrap();
        let mut executor = Executor::new(cx.runtime(), &owner, function, &[]).unwrap();
        assert!(executor.run().is_err());
        Ok(Value::Unit)
    })
    .install(&mut runtime)
    .unwrap();
    let loaded = runtime
        .load_program("reentry", round_trip_bytecode(code))
        .unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.execute(&loaded, "read").is_err());
    let StaticState::Failed(failure) = state(vm.runtime(), &loaded, "value") else {
        panic!("cycle must poison")
    };
    assert!(failure.cause().contains("recursive"));
    assert!(vm.execute(&loaded, "read").is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[derive(Debug)]
struct InitializerObserver {
    mode: u8,
}

impl ExecutionObserver for InitializerObserver {
    fn observe(
        &mut self,
        runtime: &Runtime,
        event: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        if event != ExecutionEvent::BeforeInstruction {
            return Ok(());
        }
        let root = runtime.execution_root().unwrap();
        if runtime
            .module_instance_snapshot(&root)
            .unwrap()
            .module_slots
            .iter()
            .any(|state| matches!(state, StaticState::Initializing(_)))
        {
            runtime.collect_garbage()?;
            match self.mode {
                1 => {
                    return Err(RuntimeError::module_validation(
                        "initializer observer failed",
                    ));
                }
                2 => runtime.execution_options().cancellation.cancel(),
                _ => {}
            }
        }
        Ok(())
    }
}

#[test]
fn managed_return_gc_observer_failure_and_cancellation_settle_initializer_frames() {
    let code = compile_test_bytecode(
        r#"
        static val value: [i32] = { val first = [7, 8]; first };
        static val business: Result<i32, i32> = Err(4);
        pub fn business_error() -> i32 { match business { Ok(value) => value, Err(error) => error } }
        pub fn read() -> i32 { value[0] }
    "#,
    );
    for mode in 0..3 {
        let mut runtime = standard_runtime(Default::default());
        let loaded = runtime.load_program("cleanup", code.clone()).unwrap();
        runtime
            .set_execution_observer(InitializerObserver { mode })
            .unwrap();
        let vm = Vm::new(runtime);
        let session = vm
            .runtime()
            .begin_execution(&loaded, Default::default())
            .unwrap();
        vm.runtime().attach_execution_observer().unwrap();
        let result = vm.execute(&loaded, "read");
        drop(session);
        if mode == 0 {
            assert_eq!(
                result.unwrap().return_value.value(vm.runtime().gc()),
                Some(Value::I32(7))
            );
            assert!(matches!(
                state(vm.runtime(), &loaded, "value"),
                StaticState::Ready(_)
            ));
            assert_eq!(call(&vm, &loaded, "business_error"), Value::I32(4));
            assert!(matches!(
                state(vm.runtime(), &loaded, "business"),
                StaticState::Ready(_)
            ));
        } else {
            assert!(result.is_err());
            assert!(matches!(
                state(vm.runtime(), &loaded, "value"),
                StaticState::Failed(_)
            ));
            runtime_error_is_not_new_cancellation(&vm, &loaded);
        }
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        assert_eq!(vm.runtime().gc().active_roots(), 0);
    }
}

fn runtime_error_is_not_new_cancellation(vm: &Vm, loaded: &LoadedModule) {
    let error = vm
        .runtime()
        .read_module_slot(loaded, ModuleSlot::new(0))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.message().contains("initialization failed"));
}

#[test]
fn owned_slices_never_publish_a_parked_initializing_cell() {
    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_program(
            "slices",
            compile_test_bytecode(
                r#"
        static val value: i32 = calculate();
        fn calculate() -> i32 { var n = 0; while n < 30 { n += 1; } n }
        fn main() -> i32 { value }
    "#,
            ),
        )
        .unwrap();
    let vm = Vm::new(runtime);
    let execution = vm
        .start(&loaded, ProgramEntry::Main, &[], Default::default())
        .unwrap();
    let mut complete = false;
    for _ in 0..100 {
        let progress = vm.drive(&execution, NonZeroUsize::new(1).unwrap()).unwrap();
        assert!(!matches!(
            state(vm.runtime(), &loaded, "value"),
            StaticState::Initializing(_)
        ));
        match progress {
            DriveResult::Runnable => {}
            DriveResult::Waiting => panic!("synchronous initializer cannot wait"),
            DriveResult::Complete(result) => {
                assert_eq!(
                    result.unwrap().value(vm.runtime().gc()),
                    Some(Value::I32(30))
                );
                complete = true;
                break;
            }
        }
    }
    assert!(complete);
}

#[test]
fn unsupported_native_static_entry_falls_back_before_initialization() {
    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_program(
            "fallback",
            compile_test_bytecode(
                "static var count: i32 = 0; fn main() -> i32 { count += 1; count }",
            ),
        )
        .unwrap();
    let vm = Vm::new(runtime);
    for expected in [1, 2] {
        let report = vm
            .execute_prepared(&loaded, ProgramEntry::Main, &native_fixtures::unsupported())
            .unwrap();
        assert_eq!(
            report.return_value.value(vm.runtime().gc()),
            Some(Value::I32(expected))
        );
        assert_eq!(
            report.jit.unwrap().status,
            JitExecutionStatus::InterpreterFallback
        );
    }
}

#[test]
fn indirect_native_future_construction_is_denied_without_provider_submission() {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("test::pending", &catalog).unwrap();
    let future = catalog.future_type().unwrap().apply([Type::i32()]).unwrap();
    let request = builder
        .define_function(FunctionDecl::new("request").returns(future))
        .unwrap();
    let starts = Arc::new(AtomicUsize::new(0));
    let called = starts.clone();
    builder
        .bind::<Async<()>, i32>(request, move |(), _: Completion<i32>| {
            called.fetch_add(1, Ordering::Relaxed);
            Ok(NativeStart::Ready(9))
        })
        .unwrap();
    let native = builder.finish().unwrap();
    let program = compile_native_test_bytecode(
        r#"
        use test::pending::request;
        fn helper() -> i32 { val pending = request(); 7 }
        static val value: i32 = helper();
        pub fn read() -> i32 { value }
    "#,
        &[&native],
    );
    let mut runtime = standard_runtime(Default::default());
    native.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("native_future", program).unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.execute(&loaded, "read").is_err());
    let StaticState::Failed(failure) = state(vm.runtime(), &loaded, "value") else {
        panic!("failed")
    };
    assert!(failure.cause().contains("Future"), "{}", failure.cause());
    assert_eq!(starts.load(Ordering::Relaxed), 0);
}

#[test]
fn indirect_script_future_construction_is_denied_during_initialization() {
    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_program(
            "async",
            compile_test_bytecode(
                r#"
        async fn later() -> i32 { 1 }
        fn helper() -> i32 { val pending = later(); 7 }
        static val value: i32 = helper();
        pub fn read() -> i32 { value }
    "#,
            ),
        )
        .unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.execute(&loaded, "read").is_err());
    let StaticState::Failed(failure) = state(vm.runtime(), &loaded, "value") else {
        panic!("failed")
    };
    assert!(failure.cause().contains("Future"), "{}", failure.cause());
}
