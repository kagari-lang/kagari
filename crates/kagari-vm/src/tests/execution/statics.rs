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
use kagari_bytecode::{instruction::ModuleSlot, program::BytecodeProgram};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    module::{ExecutableModule, ModuleEpochRetention, entry::ProgramEntry, state::StaticState},
    native::{
        adapter::Async, builder::ModuleBuilder, completion::Completion, declarations::FunctionDecl,
        future::NativeStart, host::HostCallback, types::Type,
    },
    reload::ReloadValidationError,
    session::{ExecutionEvent, ExecutionObserver},
    task::{CancellationCause, drive::TaskDriveResult},
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use rstest::rstest;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn call(vm: &Vm, loaded: &ExecutableModule, name: &str) -> Value {
    vm.execute(loaded, name)
        .unwrap()
        .return_value
        .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
        .unwrap()
}

fn state(runtime: &Runtime, loaded: &ExecutableModule, name: &str) -> StaticState {
    let index = loaded
        .bytecode
        .module_slots
        .iter()
        .position(|slot| slot.contract.name == name)
        .unwrap();
    unsafe { runtime.module_instance_snapshot_transport(loaded) }
        .unwrap()
        .module_slots[index]
        .clone()
}

// Keep scalar cell-state fixtures independent of the standard source catalog.
// Imported initializer dispatch also exercises the complete foundation below.
fn compile_scalar_static_bytecode(text: &str) -> DefinitionMetadata<BytecodeProgram> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let checked = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified()
}

#[test]
fn source_and_decoded_statics_are_lazy_shared_on_reload_and_keep_fixed_recipes() {
    let first = compile_scalar_static_bytecode(include_str!(
        "../../../tests/fixtures/execution/static-recipes-initial.kgr"
    ));
    let second = compile_scalar_static_bytecode(include_str!(
        "../../../tests/fixtures/execution/static-recipes-added.kgr"
    ));
    let third_source = include_str!("../../../tests/fixtures/execution/static-recipes-current.kgr");
    let third = compile_scalar_static_bytecode(third_source);
    for pattern in ["count: i32 = 0", "cold: i32 = 7", "added: i32 = 5"] {
        assert_eq!(third_source.matches(pattern).count(), 1, "{pattern}");
    }
    let changed_recipes = compile_scalar_static_bytecode(
        &third_source
            .replace("count: i32 = 0", "count: i32 = 200")
            .replace("cold: i32 = 7", "cold: i32 = 900")
            .replace("added: i32 = 5", "added: i32 = 50"),
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
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
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
            .stage_reload_program(&_old_program, Some("state".into()), code(second.clone()))
            .unwrap();
        let current = candidate.module().clone();
        vm.runtime_mut().publish_staged_reload(candidate).unwrap();
        let _current_program = _old_program.clone();
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(call(&vm, &current, "tick"), Value::I32(2));
        assert_eq!(call(&vm, &old, "tick"), Value::I32(3));
        let retained_second = vm
            .runtime()
            .retain_module(&current, ModuleEpochRetention::CompiledArtifact)
            .unwrap();
        let candidate = vm
            .runtime_mut()
            .stage_reload_program(&_current_program, Some("state".into()), code(third.clone()))
            .unwrap();
        let latest = candidate.module().clone();
        vm.runtime_mut().publish_staged_reload(candidate).unwrap();
        let _latest_program = _current_program.clone();
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(call(&vm, &latest, "tick"), Value::I32(13));
        assert_eq!(call(&vm, &old, "tick"), Value::I32(23));
        assert_eq!(call(&vm, &current, "tick"), Value::I32(33));
        assert_eq!(call(&vm, &latest, "read_cold"), Value::I32(7));
        assert_eq!(call(&vm, &latest, "read_added"), Value::I32(5));
        assert!(matches!(
            state(vm.runtime(), &current, "unused"),
            StaticState::Uninitialized(_)
        ));
        assert!(matches!(
            vm.runtime().stage_reload_program(
                &_latest_program,
                Some("state".into()),
                code(changed_recipes.clone())
            ),
            Err(ReloadValidationError::UpdateEvidence(_))
        ));
        let fresh = vm
            .runtime_mut()
            .load_program("state", code(changed_recipes.clone()))
            .unwrap();
        let _fresh_program = fresh;
        let fresh = vm.runtime().select_program(&_fresh_program).unwrap();
        assert_ne!(fresh.id, latest.id);
        assert_eq!(call(&vm, &fresh, "tick"), Value::I32(210));
        assert_eq!(call(&vm, &fresh, "read_cold"), Value::I32(900));
        assert_eq!(call(&vm, &fresh, "read_added"), Value::I32(50));
        assert!(matches!(
            state(vm.runtime(), &latest, "count"),
            StaticState::Ready(Value::I32(33))
        ));
        drop(retained_second);
        drop(retained);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    }
}

#[test]
fn candidate_indirect_reads_and_writes_cannot_touch_shared_or_private_cells() {
    let source = r#"
        static var count: i32 = 0;
        static val cold: i32 = 7;
        pub fn tick() -> i32 { count += 1; count }
        pub fn indirect_write() -> i32 { val action: fn()->i32 = || { count += 1; count }; action() }
        pub fn indirect_read() -> i32 { val action: fn()->i32 = || cold; action() }
        pub fn indirect_added() -> i32 { 0 }
    "#;
    let candidate_source = source.replace(
        "pub fn indirect_added() -> i32 { 0 }",
        "static val added: i32 = 9; pub fn indirect_added() -> i32 { val action: fn()->i32 = || added; action() }",
    );
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("candidate", compile_scalar_static_bytecode(source))
        .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(call(&vm, &old, "tick"), Value::I32(1));
    let staged = vm
        .runtime()
        .stage_reload_program(
            &_old_program,
            Some("candidate".into()),
            round_trip_bytecode(compile_scalar_static_bytecode(&candidate_source)),
        )
        .unwrap();
    {
        let _session = vm
            .runtime()
            .begin_candidate_initialization(&staged)
            .unwrap();
        for entry in ["indirect_write", "indirect_read", "indirect_added"] {
            let error = vm.execute(staged.module(), entry).unwrap_err();
            assert!(
                matches!(error.cause(), VmError::RuntimeError(error)
                    if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation
                        && error.message().contains("candidate cannot access static state")),
                "{error:?}"
            );
        }
    }
    assert!(matches!(
        state(vm.runtime(), &old, "count"),
        StaticState::Ready(Value::I32(1))
    ));
    assert!(matches!(
        state(vm.runtime(), &old, "cold"),
        StaticState::Uninitialized(_)
    ));
    let current = staged.module().clone();
    vm.runtime().publish_staged_reload(staged).unwrap();
    let _current_program = _old_program.clone();
    assert!(matches!(
        state(vm.runtime(), &current, "added"),
        StaticState::Uninitialized(_)
    ));
    assert_eq!(call(&vm, &current, "indirect_write"), Value::I32(2));
    assert_eq!(call(&vm, &current, "indirect_read"), Value::I32(7));
    assert_eq!(call(&vm, &current, "indirect_added"), Value::I32(9));
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn runtime_transfer_preserves_ready_failed_and_unstarted_recipe_ownership() {
    let source = r#"
        static var count: i32 = 0;
        static val cold: i32 = 7;
        static val bad: i32 = { val zero = 0; 1 / zero };
        pub fn tick() -> i32 { count += 1; count }
        pub fn read() -> i32 { cold }
        pub fn fail() -> i32 { bad }
    "#;
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("transfer", compile_scalar_static_bytecode(source))
        .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(call(&vm, &old, "tick"), Value::I32(1));
    assert!(vm.execute(&old, "fail").is_err());
    let staged = vm
        .runtime()
        .stage_reload_program(
            &_old_program,
            Some("transfer".into()),
            round_trip_bytecode(compile_scalar_static_bytecode(
                &source.replace("{ cold }", "{ cold + 0 }"),
            )),
        )
        .unwrap();
    let current = staged.module().clone();
    vm.runtime().publish_staged_reload(staged).unwrap();
    let _current_program = _old_program.clone();
    let vm = std::thread::spawn(move || {
        vm.runtime().collect_garbage().unwrap();
        assert!(matches!(
            state(vm.runtime(), &current, "cold"),
            StaticState::Uninitialized(_)
        ));
        assert!(matches!(
            state(vm.runtime(), &current, "bad"),
            StaticState::Failed(_)
        ));
        assert_eq!(call(&vm, &current, "tick"), Value::I32(2));
        assert_eq!(call(&vm, &current, "read"), Value::I32(7));
        assert!(vm.execute(&current, "fail").is_err());
        assert!(
            vm.runtime()
                .collect_garbage()
                .unwrap()
                .reclaimed_modules
                .contains(&old.key())
        );
        (vm, current)
    })
    .join()
    .unwrap();
    let (vm, current) = vm;
    assert_eq!(call(&vm, &current, "tick"), Value::I32(3));
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn task_cancellation_leaves_unstarted_statics_cold_and_poisoned_attempts_durable() {
    let program = compile_logging_test_bytecode(
        r#"
        static var attempts: i32 = 0;
        static val value: i32 = initialize();
        fn initialize() -> i32 { attempts += 1; print("cancel"); 42 }
        pub fn factory() -> fn()->Future<i32> { async || value }
        pub fn read() -> i32 { value }
        pub fn count() -> i32 { attempts }
    "#,
    );
    for before_start in [true, false] {
        let mut runtime = standard_runtime(Default::default());
        let options = runtime.execution_options();
        let cancellation = options.cancellation.clone();
        let cancel_during_initializer = cancellation.clone();
        logging_module(unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |cx, _| {
                cx.runtime().collect_garbage().unwrap();
                cancel_during_initializer.cancel();
                Ok(Value::Unit)
            })
        })
        .install(&mut runtime)
        .unwrap();
        let loaded = runtime
            .load_program("task_static", round_trip_bytecode(program.clone()))
            .unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let vm = Vm::new(runtime);
        let factory = vm.execute(&loaded, "factory").unwrap();
        let scope = vm
            .runtime()
            .create_task_scope(&loaded, options, Arc::new(|_| Ok(())))
            .unwrap();
        let task = unsafe {
            vm.runtime().spawn_task_transport(
                &scope
                    .capability()
                    .value_transport(vm.runtime().gc())
                    .unwrap(),
                &factory
                    .return_value
                    .value_transport(vm.runtime().gc())
                    .unwrap(),
            )
        }
        .unwrap()
        .unwrap();
        let task_id = unsafe {
            vm.runtime()
                .task_id_transport(&task.value_transport(vm.runtime().gc()).unwrap())
        }
        .unwrap();
        drop(factory);
        if before_start {
            cancellation.cancel();
        }
        let mut complete = false;
        for _ in 0..100 {
            match vm
                .drive_task(&scope, task_id, NonZeroUsize::new(1).unwrap())
                .unwrap()
            {
                TaskDriveResult::Complete => {
                    complete = true;
                    break;
                }
                TaskDriveResult::Runnable => {}
                progress => panic!("unexpected task state: {progress:?}"),
            }
            assert!(!matches!(
                state(vm.runtime(), &loaded, "value"),
                StaticState::Initializing(_)
            ));
        }
        assert!(complete);
        let failure = vm
            .runtime()
            .take_task_report(&scope, task_id)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap_err();
        assert_eq!(failure.error.kind(), RuntimeErrorKind::Cancelled);
        assert_eq!(failure.cancellation, Some(CancellationCause::ScopeClose));
        assert!(scope.is_closed());
        drop(task);
        drop(scope);
        vm.runtime().collect_garbage().unwrap();
        if before_start {
            assert!(matches!(
                state(vm.runtime(), &loaded, "value"),
                StaticState::Uninitialized(_)
            ));
            assert!(matches!(
                state(vm.runtime(), &loaded, "attempts"),
                StaticState::Uninitialized(_)
            ));
        } else {
            assert!(matches!(
                state(vm.runtime(), &loaded, "value"),
                StaticState::Failed(_)
            ));
            assert_eq!(call(&vm, &loaded, "count"), Value::I32(1));
            assert!(vm.execute(&loaded, "read").is_err());
            assert_eq!(call(&vm, &loaded, "count"), Value::I32(1));
        }
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        assert_eq!(vm.runtime().gc().active_roots(), 0);
    }
}

#[rstest]
#[case::scalar_catalog(false)]
#[case::foundation_catalog(true)]
fn imported_static_keeps_ready_values_and_calls_current_helpers_on_first_access(
    #[case] foundation: bool,
) {
    let source =
        include_str!("../../../tests/fixtures/execution/imported-static-current-helper.kgr");
    let helper = "fn helper() -> i32 {\n        7\n    }";
    assert_eq!(source.matches("value: i32 = helper()").count(), 1);
    assert_eq!(source.matches(helper).count(), 1);
    let compile = |source: &str| {
        round_trip_bytecode(if foundation {
            compile_test_bytecode(source)
        } else {
            compile_scalar_static_bytecode(source)
        })
    };
    let mut runtime = standard_runtime(Default::default());
    let program = runtime.load_program("imports", compile(source)).unwrap();
    let old = runtime.select_program(&program).unwrap();
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    let vm = Vm::new(runtime);
    assert_eq!(call(&vm, &old, "read_ready"), Value::I32(7));
    let changed_expression = source.replace("value: i32 = helper()", "value: i32 = helper() + 1");
    assert!(matches!(
        vm.runtime().stage_reload_program(
            &program,
            Some("imports".into()),
            compile(&changed_expression)
        ),
        Err(ReloadValidationError::UpdateEvidence(_))
    ));
    assert_eq!(
        vm.runtime().select_program(&program).unwrap().key(),
        old.key()
    );
    let changed_helper = source.replace(helper, "fn helper() -> i32 {\n        90\n    }");
    let staged = vm
        .runtime()
        .stage_reload_program(&program, Some("imports".into()), compile(&changed_helper))
        .unwrap();
    let current = staged.module().clone();
    vm.runtime().publish_staged_reload(staged).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(call(&vm, &current, "read_ready"), Value::I32(7));
    assert_eq!(call(&vm, &current, "read"), Value::I32(90));
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    assert_eq!(call(&vm, &current, "read"), Value::I32(90));
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
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
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
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
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
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
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
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
        let source = format!(
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
        );
        let program = compile_test_bytecode(&source);
        let repaired = compile_test_bytecode(&source.replace(
            "static val bad: i32 = initialize();",
            "static val bad: i32 = 42;",
        ));
        let repaired_helper = compile_test_bytecode(&source.replace(
            &format!("val saved = good; {initializer}"),
            "val saved = good; 42",
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
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
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
            let StaticState::Failed(original_failure) = state(vm.runtime(), &loaded, "bad") else {
                unreachable!()
            };
            assert!(matches!(
                vm.runtime().stage_reload_program(
                    &_loaded_program,
                    Some("poison".into()),
                    if decoded {
                        round_trip_bytecode(repaired.clone())
                    } else {
                        repaired.clone()
                    }
                ),
                Err(ReloadValidationError::UpdateEvidence(_))
            ));
            for candidate in [&program, &repaired_helper] {
                let staged = vm
                    .runtime_mut()
                    .stage_reload_program(
                        &_loaded_program,
                        Some("poison".into()),
                        if decoded {
                            round_trip_bytecode(candidate.clone())
                        } else {
                            candidate.clone()
                        },
                    )
                    .unwrap();
                let current = staged.module().clone();
                vm.runtime_mut().publish_staged_reload(staged).unwrap();
                vm.runtime().collect_garbage().unwrap();
                assert!(vm.execute(&current, "read").is_err());
                assert_eq!(call(&vm, &current, "count"), Value::I32(1));
                let StaticState::Failed(failure) = state(vm.runtime(), &current, "bad") else {
                    panic!("reload must preserve failure")
                };
                assert_eq!(failure.cause(), original_failure.cause());
            }
            let fresh = vm
                .runtime_mut()
                .load_program(
                    "repaired_fresh",
                    if decoded {
                        round_trip_bytecode(repaired.clone())
                    } else {
                        repaired.clone()
                    },
                )
                .unwrap();
            let _fresh_program = fresh;
            let fresh = vm.runtime().select_program(&_fresh_program).unwrap();
            assert_eq!(call(&vm, &fresh, "read"), Value::I32(42));
            assert_eq!(call(&vm, &fresh, "count"), Value::I32(0));
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
    logging_module(unsafe {
        // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
        HostCallback::new(move |cx, _| {
            called.fetch_add(1, Ordering::Relaxed);
            cx.runtime().collect_garbage().unwrap();
            let root = cx.runtime().execution_root().unwrap();
            let (owner, function) = root.exported_function("read").unwrap();
            let mut executor = Executor::new(cx.runtime(), &owner, function, &[]).unwrap();
            assert!(executor.run().is_err());
            Ok(Value::Unit)
        })
    })
    .install(&mut runtime)
    .unwrap();
    let loaded = runtime
        .load_program("reentry", round_trip_bytecode(code))
        .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
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
        if unsafe { runtime.module_instance_snapshot_transport(&root) }
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
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
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
                result
                    .unwrap()
                    .return_value
                    .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                    .ok(),
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

fn runtime_error_is_not_new_cancellation(vm: &Vm, loaded: &ExecutableModule) {
    let error = unsafe {
        vm.runtime()
            .read_module_slot_transport(loaded, ModuleSlot::new(0))
    }
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
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
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
                    result
                        .unwrap()
                        .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                        .ok(),
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
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    for expected in [1, 2] {
        let report = vm
            .execute_prepared(&loaded, ProgramEntry::Main, &native_fixtures::unsupported())
            .unwrap();
        assert_eq!(
            report
                .return_value
                .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                .ok(),
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
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
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
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.execute(&loaded, "read").is_err());
    let StaticState::Failed(failure) = state(vm.runtime(), &loaded, "value") else {
        panic!("failed")
    };
    assert!(failure.cause().contains("Future"), "{}", failure.cause());
}
