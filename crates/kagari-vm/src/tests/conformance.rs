use kagari_runtime::{
    CapabilitySet, DebugVisibilityPolicy, LanguageProfile, ResourcePolicy, Runtime, RuntimeConfig,
    RuntimeErrorKind, SecurityContext, value::StructValueField, value::Value,
};

use crate::tests::common::{compile_test_bytecode, load_test_module};
use crate::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint, Vm, VmError};

fn debug_runtime(module_name: &str) -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                debug_attach: true,
                debug_breakpoints: true,
                debug_pause: true,
                debug_stack_inspection: true,
                debug_value_inspection: true,
                debug_watch_evaluation: true,
                ..CapabilitySet::default()
            },
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec![module_name.to_owned()],
            ..DebugVisibilityPolicy::default()
        },
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
fn interpreter_conformance_classifies_failure_paths() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { val values = [1]; values[3] }");
    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("out of bounds index should trap");
    assert!(matches!(error, VmError::InvalidIndex(3)));

    let missing = vm
        .execute(&loaded, "missing")
        .expect_err("missing entry should be classified");
    assert!(matches!(missing, VmError::MissingFunction(ref name) if name == "missing"));

    let bytecode = compile_test_bytecode("fn main() -> i32 { 1 + 2 }");
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_module("resource_limit.kgr", bytecode)
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
        .load_module("debug_conformance.kgr", compile_test_bytecode(source))
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

    let pause = vm
        .debug_session()
        .expect("debug session should be attached")
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
        .load_module("debug_steps.kgr", compile_test_bytecode(source))
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
