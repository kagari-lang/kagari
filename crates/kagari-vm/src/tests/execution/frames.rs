use super::*;
use crate::tests::common::standard_runtime;
use kagari_bytecode::{
    instruction::StructId,
    program::{BytecodeProgram, ModuleRef, verify_program},
};
use kagari_runtime::error::RuntimeErrorKind;
use kagari_runtime::value_semantics::script_equal;

#[test]
fn foreign_loaded_module_is_rejected_before_execution() {
    let bytecode = compile_test_bytecode("fn main() -> i32 { 7 }");
    let mut first = standard_runtime(Default::default());
    let mut second = standard_runtime(Default::default());
    let foreign = first.load_program("same", bytecode.clone()).unwrap();
    let local = second.load_program("same", bytecode).unwrap();
    assert_eq!(foreign.key(), local.key());
    let vm = Vm::new(second);
    assert!(
        matches!(vm.execute(&foreign, "main"), Err(VmError::RuntimeError(ref error)) if error.kind() == RuntimeErrorKind::ModuleValidation)
    );
    assert_eq!(
        vm.execute(&local, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(7)
    );
}

#[test]
fn executes_simple_arithmetic_function() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { val value = 1 + 2; value }");
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(3)
    );
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(loaded.key())
            .active_calls,
        0
    );
}

#[test]
fn rejects_unverified_bytecode_before_publication() {
    let mut bytecode = verified_module(vec![test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Register::new(0),
                constant: ConstantOperand::I32(1),
            },
            BytecodeInstruction::Return(Some(Register::new(0))),
        ],
        ValueType::I32,
        vec![ValueType::I32],
    )]);
    bytecode.function_table.clear();
    let mut runtime = standard_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            ..RuntimeLimits::default()
        },
        ..RuntimeConfig::default()
    });
    let error = runtime
        .load_program(
            "unverified.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect_err("runtime must reject unverified bytecode before publication");

    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.message().contains("function table length mismatch"));
    assert_eq!(runtime.modules().loaded_count(), 0);
}

#[test]
fn rejects_unsupported_bytecode_before_publication() {
    let register_call = verified_module(vec![test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Register::new(0),
                constant: ConstantOperand::I32(1),
            },
            BytecodeInstruction::Call {
                dst: None,
                callee: CallTarget::Register(Register::new(0)),
                args: vec![],
            },
            BytecodeInstruction::Return(None),
        ],
        ValueType::Unit,
        vec![ValueType::I32],
    )]);
    let dynamic_call = verified_module(vec![test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::Call {
                dst: None,
                callee: CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall),
                args: vec![],
            },
            BytecodeInstruction::Return(None),
        ],
        ValueType::Unit,
        vec![],
    )]);
    let mut runtime = standard_runtime(Default::default());
    for (name, bytecode) in [
        ("register_call.kbc", register_call),
        ("dynamic_call.kbc", dynamic_call),
    ] {
        let error = runtime
            .load_program(
                name,
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![bytecode],
                },
            )
            .expect_err("unsupported calls must fail bytecode verification");
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        assert!(error.message().contains("invalid operation"));
        assert_eq!(runtime.modules().loaded_count(), 0);
    }
}

#[test]
fn unsupported_dynamic_invocation_is_rejected() {
    let dynamic_call = verified_module(vec![test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::Call {
                dst: None,
                callee: CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall),
                args: vec![],
            },
            BytecodeInstruction::Return(None),
        ],
        ValueType::Unit,
        vec![],
    )]);
    let mut runtime = standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    let error = runtime
        .load_program(
            "dynamic_call_capable.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![dynamic_call],
            },
        )
        .expect_err("unimplemented call forms remain rejected");
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(runtime.modules().loaded_count(), 0);
}

#[test]
fn executes_if_control_flow() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { if true { 1 } else { 2 } }");
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(1)
    );
}

#[test]
fn executes_direct_function_calls() {
    let (runtime, loaded) = load_test_module(
        r#"
fn callee() -> i32 { 7 }
fn main() -> i32 { callee() }
"#,
    );
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(7)
    );
}

#[test]
fn deterministic_frames_keep_caller_and_callee_storage_isolated() {
    let (runtime, loaded) = load_test_module(
        r#"
fn callee(input: i32) -> i32 {
    val local = input + 1;
    local
}

fn main() -> i32 {
    val local = 10;
    val returned = callee(1);
    local + returned
}
"#,
    );
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(12)
    );
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn deterministic_frames_account_call_depth_and_unwind_on_failure() {
    let bytecode = compile_test_bytecode(
        r#"
fn leaf() -> i32 { 1 }
fn middle() -> i32 { leaf() }
fn main() -> i32 { middle() }
"#,
    );
    let mut runtime = standard_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            max_call_depth: Some(2),
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program("call_depth.kgr", bytecode)
        .expect("module should load");

    let vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("third frame should exceed call depth");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref err)
            if err.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().resources().counters().peak_call_depth, 2);
}

#[test]
fn unreachable_instruction_is_a_script_trap() {
    let mut runtime = host_call_runtime();
    let loaded = runtime
        .load_program(
            "trap.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![verified_module(vec![test_function(
                    0,
                    "main",
                    vec![BytecodeInstruction::Unreachable],
                    ValueType::Unit,
                    vec![],
                )])],
            },
        )
        .expect("module should load");

    let vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("unreachable should trap");

    assert!(matches!(error.cause(), VmError::Trap("unreachable")));
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn executes_array_index_access() {
    let (runtime, loaded) =
        load_test_module("fn main() -> i32 { val values = [1, 2, 3]; values[1] }");
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(2)
    );
}

#[test]
fn executes_struct_field_access() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { var x: i32, var y: i32 }

fn main() -> i32 {
    val point = Point { x: 1, y: 2 };
    point.y
}
"#,
    );
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(2)
    );
}

#[test]
fn executes_tuple_literal_return() {
    let (runtime, loaded) = load_test_module("fn main() -> (bool, bool) { (true, false) }");
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert!(
        script_equal(
            vm.runtime().gc(),
            &(report
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result")),
            &(vm.runtime()
                .gc()
                .alloc_tuple(vec![Value::Bool(true), Value::Bool(false)])
                .unwrap())
        )
        .unwrap()
    );
}

#[test]
fn executes_struct_literal_return() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { var x: i32, var y: i32 }

fn main() -> Point {
    Point { x: 1, y: 2 }
}
"#,
    );
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    let Value::Struct(handle) = report
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("expected struct return value");
    };
    assert_eq!(
        vm.runtime().gc().struct_snapshot(handle),
        Some((
            "Point".to_owned(),
            vec![
                StructValueField {
                    name: "x".to_owned(),
                    value: Value::I32(1),
                },
                StructValueField {
                    name: "y".to_owned(),
                    value: Value::I32(2),
                },
            ],
        ))
    );
}

#[test]
fn module_slot_driver_and_checked_host_writes_share_storage() {
    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_program(
            "module-slot-writes",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module_with_mutable_slot(7)],
            },
        )
        .unwrap();
    let vm = Vm::new(runtime);
    vm.execute(&loaded, "init").unwrap();
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(7)
    );
    let slot = ModuleSlot::new(0);
    assert!(
        vm.runtime()
            .write_module_slot(&loaded, slot, Value::Bool(true))
            .is_err()
    );
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(7)
    );
    vm.runtime()
        .write_module_slot(&loaded, slot, Value::I32(9))
        .unwrap();
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(9)
    );
    assert!(!vm.runtime().is_quarantined());
    assert!(vm.runtime().execution_root().is_none());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn reload_preserves_active_old_epoch_while_new_calls_use_latest_epoch() {
    let mut runtime = standard_runtime(Default::default());
    let first_loaded = runtime
        .load_program(
            "hot_reload.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![reloadable_value_module(1)],
            },
        )
        .expect("first module epoch should load");
    let retention = runtime
        .retain_module(&first_loaded, ModuleEpochRetention::ActiveCall)
        .unwrap();

    let second_loaded = runtime
        .stage_reload_program(
            &first_loaded,
            "hot_reload.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![reloadable_value_module(2)],
            },
        )
        .expect("compatible reload should publish a new epoch");
    let second_loaded = runtime.publish_staged_reload(second_loaded).unwrap();

    assert_eq!(
        runtime.collect_garbage().unwrap().reclaimed_modules,
        Vec::new(),
        "old active-call epoch must remain reachable after reload"
    );

    let vm = Vm::new(runtime);
    let old_report = vm
        .execute(&first_loaded, "main")
        .expect("old active epoch should remain executable");
    let latest = vm
        .runtime()
        .modules()
        .latest("hot_reload.kgr")
        .expect("latest module epoch should be visible");
    let latest_report = vm
        .execute(&latest, "main")
        .expect("new call should use latest epoch");

    assert_eq!(second_loaded.epoch, latest.epoch);
    assert_eq!(old_report.epoch, first_loaded.epoch.0);
    assert_eq!(
        old_report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(1)
    );
    assert_eq!(latest_report.epoch, second_loaded.epoch.0);
    assert_eq!(
        latest_report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(2)
    );

    drop(retention);
    assert_eq!(
        vm.runtime().collect_garbage().unwrap().reclaimed_modules,
        vec![first_loaded.key()]
    );
}

#[test]
fn aggregate_field_instructions_reject_a_different_nominal_receiver() {
    // Semantic receiver contracts reject layout substitution before execution.
    for write in [false, true] {
        let mut bytecode = compile_test_bytecode(
            "struct P { var x: i32 } struct Q { var x: i32 } fn main() -> i32 { val p = P { x: 1 }; p.x = 42; p.x }",
        );
        let wrong = StructId::new(
            bytecode.modules[bytecode.root.index()]
                .structures
                .iter()
                .position(|layout| layout.name() == "Q")
                .unwrap(),
        );
        for instruction in bytecode.modules[bytecode.root.index()]
            .functions
            .iter_mut()
            .flat_map(|function| &mut function.instructions)
        {
            match instruction {
                BytecodeInstruction::WriteAggregateField { field, .. } if write => {
                    field.structure = wrong
                }
                BytecodeInstruction::ReadAggregateField { field, .. } if !write => {
                    field.structure = wrong
                }
                _ => {}
            }
        }
        assert!(verify_program(&bytecode).is_err());
    }
}
