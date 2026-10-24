use crate::{
    debug::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint},
    error::VmError,
    tests::{
        common::{compile_test_bytecode, load_test_module, round_trip_bytecode, standard_runtime},
        native_fixtures,
    },
    vm::Vm,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    module::entry::ProgramEntry,
    resource::RuntimeLimits,
    value::{StructValueField, Value},
};

fn debug_runtime(_module_name: &str) -> Runtime {
    standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

#[test]
fn interpreter_conformance_executes_control_flow_match_arrays_and_structs() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../tests/fixtures/execution/control-flow-aggregates.kgr"
    ));
    let vm = Vm::new(runtime);
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(13)
    );
}

#[test]
fn missing_entry_is_rejected_before_execution() {
    let bytecode = compile_test_bytecode("fn main() -> i32 { 42 }");
    for encoded in [false, true] {
        let program = bytecode.clone();
        let program = if encoded {
            round_trip_bytecode(program)
        } else {
            program
        };
        let mut runtime = standard_runtime(Default::default());
        let loaded = runtime.load_program("missing-entry.kgr", program).unwrap();
        let vm = Vm::new(runtime);
        for jit in [false, true] {
            let error = if jit {
                let prepared = native_fixtures::unsupported();
                vm.execute_prepared(&loaded, "missing", &prepared)
                    .unwrap_err()
            } else {
                vm.execute(&loaded, "missing").unwrap_err()
            };
            assert!(
                matches!(error, VmError::MissingFunction(ref name) if name == "missing"),
                "{encoded}/{jit}: {error:?}"
            );

            assert_eq!(vm.runtime().gc().active_roots(), 0);
        }
    }
}

#[test]
fn public_entries_use_verified_identity_despite_duplicate_display_names() {
    let (definitions, mut bytecode) =
        compile_test_bytecode("pub fn first() -> i32 { 1 } pub fn second() -> i32 { 2 }")
            .into_parts();
    let second = bytecode.modules[bytecode.root.index()]
        .functions
        .iter()
        .position(|function| function.name == "second")
        .unwrap();
    bytecode.modules[bytecode.root.index()].functions[second].name = "first".into();
    bytecode.modules[bytecode.root.index()].function_table[second].name = "first".into();
    let bytecode = DefinitionMetadata::checked(definitions, bytecode, &Default::default()).unwrap();
    for encoded in [false, true] {
        let program = bytecode.clone();
        let program = if encoded {
            round_trip_bytecode(program)
        } else {
            program
        };
        let mut runtime = standard_runtime(Default::default());
        let loaded = runtime
            .load_program("ambiguous-entry.kgr", program)
            .unwrap();
        let vm = Vm::new(runtime);
        for jit in [false, true] {
            for (path, expected) in [("first", 1), ("second", 2)] {
                let report = if jit {
                    let prepared = native_fixtures::unsupported();
                    vm.execute_prepared(&loaded, path, &prepared).unwrap()
                } else {
                    vm.execute(&loaded, path).unwrap()
                };
                assert_eq!(
                    report.return_value.value(vm.runtime().gc()),
                    Some(Value::I32(expected)),
                    "{encoded}/{jit}/{path}"
                );
                drop(report);
                assert_eq!(vm.runtime().gc().active_roots(), 0);
            }
        }
    }
}

#[test]
fn interpreter_conformance_classifies_failure_paths() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { val values = [1]; values[3] }");
    let vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect_err("out of bounds index should trap");
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::IndexOutOfBounds)
    );

    let missing = vm
        .execute(&loaded, "missing")
        .expect_err("missing entry should be classified");
    assert!(matches!(missing, VmError::MissingFunction(ref name) if name == "missing"));

    let bytecode = compile_test_bytecode("fn main() -> i32 { 1 + 2 }");
    let mut runtime = standard_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            max_call_depth: Some(0),
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program("resource_limit.kgr", bytecode)
        .expect("module should load");
    let vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect_err("resource limit should be classified");
    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
}

#[test]
fn machine_boundaries_preserve_wide_indices_and_repeat_resource_errors() {
    let (runtime, loaded) = load_test_module(
        r#"
        pub fn read(index: u64) -> i32 { val values = [7]; values[index] }
        pub fn write(index: u64) { val values = [7]; values[index] = 9; }
        pub fn signed(index: i64) -> i32 { val values = [7]; values[index] }
        pub fn repeat(count: u64) -> u64 { val values = [7u64; count]; values.len() }
        pub fn make() -> [i32] { [0, 7] }
        fn base(values: [i32]) -> [i32] { values[0] = values[0] * 10 + 1; values }
        fn index(values: [i32], position: u64) -> u64 { values[0] = values[0] * 10 + 2; position }
        fn replacement(values: [i32]) -> i32 { values[0] = values[0] * 10 + 3; 9 }
        pub fn write_order(values: [i32], position: u64) {
            base(values)[index(values, position)] = replacement(values);
        }
    "#,
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute_typed::<_, i32>(&loaded, "read", (0_u64,))
            .unwrap(),
        7
    );
    assert_eq!(
        vm.execute_typed::<_, u64>(&loaded, "repeat", (1_u64,))
            .unwrap(),
        1
    );
    for index in [1_u64 << 32, u64::MAX] {
        for name in ["read", "write"] {
            let error = if name == "read" {
                vm.execute_typed::<_, i32>(&loaded, name, (index,))
                    .unwrap_err()
            } else {
                vm.execute_typed::<_, ()>(&loaded, name, (index,))
                    .unwrap_err()
            };
            if name == "write" {
                assert!(
                    matches!(error.cause(), VmError::InvalidIndex(actual) if *actual == index),
                    "{error:?}"
                );
            } else {
                assert!(
                    matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::IndexOutOfBounds && error.message().contains(&index.to_string())),
                    "{error:?}"
                );
            }
        }
    }
    let error = vm
        .execute_typed::<_, i32>(&loaded, "signed", (1_i64 << 32,))
        .unwrap_err();
    assert!(
        matches!(error.cause(), VmError::InvalidIndex(4_294_967_296)),
        "{error:?}"
    );
    for count in [u64::MAX, isize::MAX as u64 / 8 + 1] {
        let error = vm
            .execute_typed::<_, u64>(&loaded, "repeat", (count,))
            .unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded),
            "{error:?}"
        );
    }
    for index in [1_u64 << 32, u64::MAX] {
        let report = vm.execute(&loaded, "make").unwrap();
        let value = report.return_value.value(vm.runtime().gc()).unwrap();
        let error = vm
            .runtime_mut()
            .with_host_scope(|scope| {
                Vm::execute_scoped(
                    scope,
                    &loaded,
                    "write_order",
                    &[value, Value::U64(index)],
                    Default::default(),
                )
            })
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::IndexOutOfBounds);
        assert!(error.message().contains(&index.to_string()));
        let Value::Array(id) = value else {
            unreachable!()
        };
        assert_eq!(
            vm.runtime().gc().array_snapshot(id),
            Some(vec![Value::I32(123), Value::I32(7)])
        );
    }
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn interpreter_debug_conformance_covers_stack_values_and_watch_expressions() {
    let source = r#"
struct Point { var x: i32, var y: i32 }

fn callee(input: i32) -> i32 {
    val doubled = input + input;
    val numbers = [input, doubled];
    val point = Point { x: numbers[0], y: doubled };
    point.x + point.y
}

fn main() -> i32 {
    val seed = 4;
    callee(seed)
}
"#;
    let mut runtime = debug_runtime("debug_conformance.kgr");
    let loaded = runtime
        .load_program("debug_conformance.kgr", compile_test_bytecode(source))
        .expect("module should load");
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    let breakpoint = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug_conformance.kgr",
            source
                .find("point.x")
                .expect("source should contain tail expr"),
        ))
        .expect("breakpoint should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("vm should execute");
    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(12)
    );

    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
    let pause = debug
        .pauses()
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(breakpoint))
        .expect("breakpoint should pause");
    assert_eq!(pause.frames.len(), 2);
    let caller = &pause.frames[0];
    let callee = pause.top_frame().expect("pause should expose top frame");
    assert_eq!(caller.function_name, "main");
    assert_eq!(callee.function_name, "callee");
    assert_eq!(
        pause
            .evaluate_watch(
                vm.runtime(),
                callee.id,
                &DebugWatch::Binding("doubled".to_owned()),
            )
            .expect("watch should read live local"),
        Value::I32(8)
    );
    assert!(matches!(
        pause
            .evaluate_watch(
                vm.runtime(),
                callee.id,
                &DebugWatch::Binding("missing".to_owned())
            )
            .expect_err("missing watch binding should be classified"),
        VmError::MissingField(ref name) if name == "missing"
    ));

    let numbers = callee
        .bindings
        .iter()
        .find(|binding| binding.name == "numbers")
        .expect("numbers binding should be inspectable")
        .value;
    let point = callee
        .bindings
        .iter()
        .find(|binding| binding.name == "point")
        .expect("point binding should be inspectable")
        .value;
    let Value::Array(numbers) = numbers else {
        panic!("expected array binding");
    };
    let Value::Struct(point) = point else {
        panic!("expected struct binding");
    };

    assert_eq!(
        vm.runtime().gc().array_snapshot(numbers),
        Some(vec![Value::I32(4), Value::I32(8)])
    );
    assert_eq!(
        vm.runtime().gc().struct_snapshot(point),
        Some((
            "Point".to_owned(),
            vec![
                StructValueField {
                    name: "x".to_owned(),
                    value: Value::I32(4),
                },
                StructValueField {
                    name: "y".to_owned(),
                    value: Value::I32(8),
                },
            ],
        ))
    );
}

#[test]
fn interpreter_debug_conformance_covers_stepping_and_run_to_cursor() {
    let source = r#"
fn helper(value: i32) -> i32 {
    value + 1
}

fn main() -> i32 {
    val seed = 2;
    helper(seed)
}
"#;
    let mut runtime = debug_runtime("debug_steps.kgr");
    let loaded = runtime
        .load_program("debug_steps.kgr", compile_test_bytecode(source))
        .expect("module should load");
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    let cursor = session
        .run_to_cursor(
            "debug_steps.kgr",
            source
                .find("helper(seed)")
                .expect("source should contain cursor target"),
        )
        .expect("run to cursor should be allowed");
    session.step_into().expect("step should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(3)
    );
    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
    assert!(
        debug
            .pauses()
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Step)
    );
    assert!(
        debug
            .pauses()
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Breakpoint(cursor))
    );
    assert!(
        !debug
            .resolved_breakpoints()
            .iter()
            .any(|breakpoint| breakpoint.breakpoint_id == cursor),
        "temporary run-to-cursor breakpoint should clear after it is hit"
    );
}
