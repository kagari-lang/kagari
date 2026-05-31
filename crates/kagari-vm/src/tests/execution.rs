use std::sync::{Arc, Mutex};

use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget, ConstantOperand,
    FunctionMetadata, FunctionRecord, FunctionRef, Register, RuntimeHelper,
};
use kagari_ir::module::ValueType;
use kagari_runtime::host::{HostFunction, HostFunctionMetadata};
use kagari_runtime::value::{StructValueField, Value};
use kagari_runtime::{CapabilitySet, ResourcePolicy, Runtime, RuntimeConfig, RuntimeErrorKind};

use crate::tests::common::{compile_test_bytecode, load_test_module};
use crate::{Vm, VmError};

fn test_function(
    id: usize,
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
) -> BytecodeFunction {
    let metadata = FunctionMetadata {
        return_type,
        registers,
        ..FunctionMetadata::default()
    };
    BytecodeFunction {
        id: FunctionRef::new(id),
        name: name.to_owned(),
        parameter_count: 0,
        register_count: metadata.registers.len() as u16,
        local_count: 0,
        metadata,
        instructions,
    }
}

fn verified_module(
    module_init: Option<FunctionRef>,
    functions: Vec<BytecodeFunction>,
) -> BytecodeModule {
    let constants = functions
        .iter()
        .flat_map(|function| &function.instructions)
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::LoadConst { constant, .. } => Some(constant.clone()),
            _ => None,
        })
        .fold(Vec::new(), |mut constants, constant| {
            if !constants.contains(&constant) {
                constants.push(constant);
            }
            constants
        });
    let mut types = vec![ValueType::Unit];
    for function in &functions {
        for ty in std::iter::once(function.metadata.return_type)
            .chain(function.metadata.params.iter().copied())
            .chain(function.metadata.locals.iter().copied())
            .chain(function.metadata.registers.iter().copied())
        {
            if !types.contains(&ty) {
                types.push(ty);
            }
        }
    }
    let function_table = functions
        .iter()
        .map(|function| FunctionRecord {
            id: function.id,
            name: function.name.clone(),
            params: function.metadata.params.clone(),
            return_type: function.metadata.return_type,
            effects: function.metadata.effects,
        })
        .collect();
    BytecodeModule {
        module_init,
        constants,
        types,
        function_table,
        functions,
        ..BytecodeModule::default()
    }
}

#[test]
fn executes_simple_arithmetic_function() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { val value = 1 + 2; value }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(3));
}

#[test]
fn reports_runtime_instruction_step_limit() {
    let bytecode = compile_test_bytecode("fn main() -> i32 { 1 }");
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_module("limited.kgr", bytecode)
        .expect("limited module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("execution should hit instruction step limit");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref err)
            if err.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
}

#[test]
fn rejects_unverified_bytecode_before_execution() {
    let mut bytecode = verified_module(
        None,
        vec![test_function(
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
        )],
    );
    bytecode.function_table.clear();
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_instruction_steps: Some(0),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_module("unverified.kbc", bytecode)
        .expect("runtime can store bytecode before VM validation");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("VM must reject unverified bytecode before dispatch");

    assert!(matches!(
        error,
        VmError::BytecodeVerification(
            kagari_ir::bytecode::BytecodeVerificationError::FunctionTableLengthMismatch {
                functions: 1,
                table: 0,
            }
        )
    ));
}

#[test]
fn rejects_unsupported_bytecode_before_execution() {
    let register_call = verified_module(
        None,
        vec![test_function(
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
        )],
    );
    let dynamic_call = verified_module(
        None,
        vec![test_function(
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
        )],
    );
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_instruction_steps: Some(0),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let register_loaded = runtime
        .load_module("register_call.kbc", register_call)
        .expect("runtime can store bytecode before VM support validation");
    let dynamic_loaded = runtime
        .load_module("dynamic_call.kbc", dynamic_call)
        .expect("runtime can store bytecode before VM support validation");
    let mut vm = Vm::new(runtime);

    assert!(matches!(
        vm.execute(&register_loaded, "main").unwrap_err(),
        VmError::UnsupportedCallTarget(CallTarget::Register(_))
    ));
    assert!(matches!(
        vm.execute(&dynamic_loaded, "main").unwrap_err(),
        VmError::UnsupportedInstruction("dynamic_call")
    ));
}

#[test]
fn executes_if_control_flow() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { if true { 1 } else { 2 } }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(1));
}

#[test]
fn executes_direct_function_calls() {
    let (runtime, loaded) = load_test_module(
        r#"
fn callee() -> i32 { 7 }
fn main() -> i32 { callee() }
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(7));
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
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(12));
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
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_call_depth: Some(2),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_module("call_depth.kgr", bytecode)
        .expect("module should load");

    let mut vm = Vm::new(runtime);
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
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_module(
            "trap.kbc",
            verified_module(
                None,
                vec![test_function(
                    0,
                    "main",
                    vec![BytecodeInstruction::Unreachable],
                    ValueType::Unit,
                    vec![],
                )],
            ),
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("unreachable should trap");

    assert!(matches!(error, VmError::Trap("unreachable")));
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn executes_array_index_access() {
    let (runtime, loaded) =
        load_test_module("fn main() -> i32 { val values = [1, 2, 3]; values[1] }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(2));
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
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(2));
}

#[test]
fn executes_tuple_literal_return() {
    let (runtime, loaded) = load_test_module("fn main() -> (bool, bool) { (true, false) }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![Value::Bool(true), Value::Bool(false)])
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
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    let Value::Struct(handle) = report.return_value else {
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
fn executes_top_level_tail_expression_as_module_result() {
    let (runtime, loaded) = load_test_module(
        r#"
val value = 1;

value + 2
"#,
    );
    let mut vm = Vm::new(runtime);
    let result = vm
        .execute_module(&loaded)
        .expect("module init should execute");

    assert_eq!(result, Value::I32(3));
}

#[test]
fn host_runtime_helpers_enforce_capability_requirements_before_invocation() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = Arc::clone(&calls);
    let mut metadata = HostFunctionMetadata::new("host.secure", vec![], "i32");
    metadata.capability_requirements = CapabilitySet {
        fs_read: true,
        ..CapabilitySet::default()
    };

    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::with_metadata(metadata, move |_| {
            *calls_for_host
                .lock()
                .expect("host call counter should lock") += 1;
            Ok(Value::I32(1))
        }))
        .expect("host function should register");
    let loaded = runtime
        .load_module(
            "host_capability.kbc",
            verified_module(
                None,
                vec![test_function(
                    0,
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                                "host.secure".to_owned(),
                            )),
                            args: vec![],
                        },
                        BytecodeInstruction::Return(Some(Register::new(0))),
                    ],
                    ValueType::I32,
                    vec![ValueType::I32],
                )],
            ),
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("host helper should be denied");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("fs_read")
    ));
    assert_eq!(*calls.lock().expect("host call counter should lock"), 0);
}

#[test]
fn host_runtime_helpers_charge_resource_cost_before_invocation() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = Arc::clone(&calls);
    let mut metadata = HostFunctionMetadata::new("host.costly", vec![], "i32");
    metadata.resource_cost_hint = Some(2);

    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_instruction_steps: Some(2),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    runtime
        .register_host_function(HostFunction::with_metadata(metadata, move |_| {
            *calls_for_host
                .lock()
                .expect("host call counter should lock") += 1;
            Ok(Value::I32(1))
        }))
        .expect("host function should register");
    let loaded = runtime
        .load_module(
            "host_cost.kbc",
            verified_module(
                None,
                vec![test_function(
                    0,
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                                "host.costly".to_owned(),
                            )),
                            args: vec![],
                        },
                        BytecodeInstruction::Return(Some(Register::new(0))),
                    ],
                    ValueType::I32,
                    vec![ValueType::I32],
                )],
            ),
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("host helper should hit cost limit");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
    assert_eq!(*calls.lock().expect("host call counter should lock"), 0);
}

#[test]
fn executes_module_init_before_entry_only_once_per_module_epoch() {
    let init_count = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&init_count);

    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new(
            "host.bump_init",
            vec![],
            "()",
            move |_| {
                let mut count = counter.lock().expect("counter lock should succeed");
                *count += 1;
                Ok(Value::Unit)
            },
        ))
        .expect("host function should register");

    let loaded = runtime
        .load_module(
            "module_init_once.kgr",
            verified_module(
                Some(FunctionRef::new(0)),
                vec![
                    test_function(
                        0,
                        "__module_init__",
                        vec![
                            BytecodeInstruction::Call {
                                dst: None,
                                callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                                    "host.bump_init".to_owned(),
                                )),
                                args: vec![],
                            },
                            BytecodeInstruction::Return(None),
                        ],
                        ValueType::Unit,
                        vec![],
                    ),
                    test_function(
                        1,
                        "main",
                        vec![
                            BytecodeInstruction::LoadConst {
                                dst: Register::new(0),
                                constant: ConstantOperand::I32(7),
                            },
                            BytecodeInstruction::Return(Some(Register::new(0))),
                        ],
                        ValueType::I32,
                        vec![ValueType::I32],
                    ),
                ],
            ),
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let first = vm
        .execute(&loaded, "main")
        .expect("first execution should work");
    let second = vm
        .execute(&loaded, "main")
        .expect("second execution should work");

    assert_eq!(first.return_value, Value::I32(7));
    assert_eq!(second.return_value, Value::I32(7));
    assert_eq!(*init_count.lock().expect("counter lock should succeed"), 1);
}

#[test]
fn reruns_module_init_for_new_module_epoch() {
    let init_count = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&init_count);

    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new(
            "host.bump_init",
            vec![],
            "()",
            move |_| {
                let mut count = counter.lock().expect("counter lock should succeed");
                *count += 1;
                Ok(Value::Unit)
            },
        ))
        .expect("host function should register");

    let bytecode = verified_module(
        Some(FunctionRef::new(0)),
        vec![test_function(
            0,
            "__module_init__",
            vec![
                BytecodeInstruction::Call {
                    dst: None,
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                        "host.bump_init".to_owned(),
                    )),
                    args: vec![],
                },
                BytecodeInstruction::Return(None),
            ],
            ValueType::Unit,
            vec![],
        )],
    );
    let first_loaded = runtime
        .load_module("reloadable.kgr", bytecode.clone())
        .expect("first module epoch should load");
    let second_loaded = runtime
        .load_module("reloadable.kgr", bytecode)
        .expect("second module epoch should load");

    let mut vm = Vm::new(runtime);
    vm.execute_module(&first_loaded)
        .expect("first module epoch should initialize");
    vm.execute_module(&second_loaded)
        .expect("second module epoch should initialize");

    assert_eq!(*init_count.lock().expect("counter lock should succeed"), 2);
}

#[test]
fn caches_failed_module_init_without_retrying() {
    let init_count = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&init_count);

    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new(
            "host.fail_init",
            vec![],
            "()",
            move |_| {
                let mut count = counter.lock().expect("counter lock should succeed");
                *count += 1;
                Err(kagari_runtime::host::HostError::new("boom"))
            },
        ))
        .expect("host function should register");

    let loaded = runtime
        .load_module(
            "module_init_failed.kgr",
            verified_module(
                Some(FunctionRef::new(0)),
                vec![test_function(
                    0,
                    "__module_init__",
                    vec![
                        BytecodeInstruction::Call {
                            dst: None,
                            callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                                "host.fail_init".to_owned(),
                            )),
                            args: vec![],
                        },
                        BytecodeInstruction::Return(None),
                    ],
                    ValueType::Unit,
                    vec![],
                )],
            ),
        )
        .expect("failed-init module should load");

    let mut vm = Vm::new(runtime);
    let first = vm
        .execute_module(&loaded)
        .expect_err("module init should fail");
    let second = vm
        .execute_module(&loaded)
        .expect_err("failed module should stay failed");

    assert!(matches!(first, VmError::HostError(ref err) if err.message() == "boom"));
    assert!(matches!(second, VmError::HostError(ref err) if err.message() == "boom"));
    assert_eq!(*init_count.lock().expect("counter lock should succeed"), 1);
}
