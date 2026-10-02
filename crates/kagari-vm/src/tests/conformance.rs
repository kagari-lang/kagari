use crate::{
    debug::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint},
    error::VmError,
    tests::{
        common::{compile_test_bytecode, load_test_module},
        native_fixtures,
    },
    vm::Vm,
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    resource::RuntimeLimits,
    value::{StructValueField, Value},
};

fn debug_runtime(_module_name: &str) -> Runtime {
    Runtime::new(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

#[test]
fn interpreter_conformance_executes_control_flow_match_arrays_and_structs() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { var x: i32, var y: i32 }

fn main() -> i32 {
    var total = 0;
    var index = 0;
    while index < 3 {
        total = total + index;
        index = index + 1;
    }
    val values = [total, 10];
    val selected = match values[1] { 10 => values[1], _ => 0 };
    val point = Point { x: values[0], y: selected };
    point.x + point.y
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(13));
}

#[test]
fn missing_entry_is_rejected_before_execution() {
    use kagari_bytecode::artifact::KbcArtifact;
    let bytecode = compile_test_bytecode("fn main() -> i32 { 42 }");
    for encoded in [false, true] {
        let program = bytecode.clone();
        let program = if encoded {
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
            decoded.validate_for_loader(&Default::default()).unwrap();
            decoded.program
        } else {
            program
        };
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("missing-entry.kgr", program).unwrap();
        let mut vm = Vm::new(runtime);
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
fn ambiguous_entry_is_rejected_on_all_load_routes() {
    use kagari_bytecode::artifact::KbcArtifact;
    let mut bytecode = compile_test_bytecode("fn first() -> i32 { 1 } fn second() -> i32 { 2 }");
    let second = bytecode.modules[bytecode.root.index()]
        .functions
        .iter()
        .position(|function| function.name == "second")
        .unwrap();
    bytecode.modules[bytecode.root.index()].functions[second].name = "first".into();
    bytecode.modules[bytecode.root.index()].function_table[second].name = "first".into();
    for encoded in [false, true] {
        let program = bytecode.clone();
        let program = if encoded {
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
            decoded.validate_for_loader(&Default::default()).unwrap();
            decoded.program
        } else {
            program
        };
        let mut runtime = Runtime::default();
        let loaded = runtime
            .load_program("ambiguous-entry.kgr", program)
            .unwrap();
        let mut vm = Vm::new(runtime);
        for jit in [false, true] {
            let error = if jit {
                let prepared = native_fixtures::unsupported();
                vm.execute_prepared(&loaded, "first", &prepared)
                    .unwrap_err()
            } else {
                vm.execute(&loaded, "first").unwrap_err()
            };
            assert!(
                matches!(error, VmError::AmbiguousFunction(ref name) if name == "first"),
                "{encoded}/{jit}: {error:?}"
            );

            assert_eq!(vm.runtime().gc().active_roots(), 0);
        }
    }
}

#[test]
fn interpreter_conformance_classifies_failure_paths() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { val values = [1]; values[3] }");
    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("out of bounds index should trap");
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::IndexOutOfBounds)
    );

    let missing = vm
        .execute(&loaded, "missing")
        .expect_err("missing entry should be classified");
    assert!(matches!(missing, VmError::MissingFunction(ref name) if name == "missing"));

    let bytecode = compile_test_bytecode("fn main() -> i32 { 1 + 2 }");
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            max_call_depth: Some(0),
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program("resource_limit.kgr", bytecode)
        .expect("module should load");
    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("resource limit should be classified");
    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
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
    let report = vm.execute(&loaded, "main").expect("vm should execute");
    assert_eq!(report.return_value, Value::I32(12));

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
        .value
        .clone();
    let point = callee
        .bindings
        .iter()
        .find(|binding| binding.name == "point")
        .expect("point binding should be inspectable")
        .value
        .clone();
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
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(3));
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
