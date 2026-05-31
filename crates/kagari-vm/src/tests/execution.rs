use std::sync::{Arc, Mutex};

use kagari_common::Span;
use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeInstruction, BytecodeModule, BytecodeModuleSlot, CallTarget,
    ConstantOperand, DebugPointId, FunctionMetadata, FunctionRecord, FunctionRef,
    InstructionSourceSpan, ModuleSlot, Register, RuntimeHelper, SafeDebugPoint, SafeDebugPointKind,
};
use kagari_ir::module::ValueType;
use kagari_runtime::host::{HostFunction, HostFunctionMetadata};
use kagari_runtime::value::{StructValueField, Value};
use kagari_runtime::{
    CapabilitySet, ModuleEpochRetention, ModuleInitializationState, ResourcePolicy, Runtime,
    RuntimeConfig, RuntimeErrorKind,
};

use crate::tests::common::{compile_test_bytecode, load_test_module};
use crate::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint, Vm, VmError};

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

fn module_with_private_init_slot(value: i32) -> BytecodeModule {
    let mut module = verified_module(
        Some(FunctionRef::new(0)),
        vec![
            test_function(
                0,
                "__module_init__",
                vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantOperand::I32(value),
                    },
                    BytecodeInstruction::StoreModule {
                        slot: ModuleSlot::new(0),
                        src: Register::new(0),
                    },
                    BytecodeInstruction::Return(Some(Register::new(0))),
                ],
                ValueType::I32,
                vec![ValueType::I32],
            ),
            test_function(
                1,
                "main",
                vec![
                    BytecodeInstruction::LoadModule {
                        dst: Register::new(0),
                        slot: ModuleSlot::new(0),
                    },
                    BytecodeInstruction::Return(Some(Register::new(0))),
                ],
                ValueType::I32,
                vec![ValueType::I32],
            ),
        ],
    );
    module.module_slots = vec![BytecodeModuleSlot {
        name: "private".to_owned(),
        ty: ValueType::I32,
        mutable: false,
    }];
    module
}

fn reloadable_value_module(value: i32) -> BytecodeModule {
    verified_module(
        None,
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(value),
                },
                BytecodeInstruction::Return(Some(Register::new(0))),
            ],
            ValueType::I32,
            vec![ValueType::I32],
        )],
    )
}

#[test]
fn executes_simple_arithmetic_function() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { val value = 1 + 2; value }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(3));
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(loaded.key())
            .active_calls,
        0
    );
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
fn rejects_unverified_bytecode_before_publication() {
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
    let error = runtime
        .load_module("unverified.kbc", bytecode)
        .expect_err("runtime must reject unverified bytecode before publication");

    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.message().contains("FunctionTableLengthMismatch"));
    assert_eq!(runtime.modules().loaded_count(), 0);
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
fn debug_session_resolves_breakpoints_and_inspects_live_locals() {
    let source = r#"
fn main() -> i32 {
    val value = 3;
    value + 4
}
"#;
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_module("debug.kgr", compile_test_bytecode(source))
        .expect("debug module should load");
    let mut session = DebugSession::new();
    let breakpoint_id = session.add_breakpoint(SourceBreakpoint::at_source_offset(
        "debug.kgr",
        source
            .find("value +")
            .expect("source should contain tail expr"),
    ));

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session);
    let report = vm
        .execute(&loaded, "main")
        .expect("debugged function should execute");

    assert_eq!(report.return_value, Value::I32(7));
    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
    assert!(
        debug
            .resolved_breakpoints()
            .iter()
            .any(|breakpoint| breakpoint.breakpoint_id == breakpoint_id)
    );
    let pause = debug
        .pauses()
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(breakpoint_id))
        .expect("breakpoint should pause execution");
    let frame = pause.top_frame().expect("pause should expose a frame");
    assert_eq!(frame.function_name, "main");
    assert_eq!(
        pause
            .evaluate_watch(frame.id, &DebugWatch::Binding("value".to_owned()))
            .expect("watch should read live local"),
        Value::I32(3)
    );
}

#[test]
fn debug_session_supports_step_into_and_trap_pause_events() {
    let mut runtime = Runtime::default();
    let mut main = test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Register::new(0),
                constant: ConstantOperand::I32(1),
            },
            BytecodeInstruction::Unreachable,
        ],
        ValueType::I32,
        vec![ValueType::I32],
    );
    main.metadata.debug.source_spans = vec![
        InstructionSourceSpan {
            instruction_offset: 0,
            span: Span::new(1, 2),
        },
        InstructionSourceSpan {
            instruction_offset: 1,
            span: Span::new(3, 4),
        },
    ];
    main.metadata.debug.safe_debug_points = vec![
        SafeDebugPoint {
            id: DebugPointId::new(0),
            instruction_offset: 0,
            span: Span::new(1, 2),
            kind: SafeDebugPointKind::FunctionEntry,
        },
        SafeDebugPoint {
            id: DebugPointId::new(1),
            instruction_offset: 1,
            span: Span::new(3, 4),
            kind: SafeDebugPointKind::Trap,
        },
    ];
    let loaded = runtime
        .load_module("debug_trap.kbc", verified_module(None, vec![main]))
        .expect("trap module should load");
    let mut session = DebugSession::new();
    session.step_into();

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("unreachable should trap");

    assert!(matches!(error, VmError::Trap("unreachable")));
    let pauses = vm
        .debug_session()
        .expect("debug session should be attached")
        .pauses();
    assert!(
        pauses
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Step)
    );
    assert!(
        pauses
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Trap)
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
fn rejects_reentrant_module_result_access_while_initializing() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_module("initializing.kgr", module_with_private_init_slot(1))
        .expect("module should load");
    {
        let mut instance = runtime
            .module_instance_mut(&loaded)
            .expect("module instance should exist");
        instance.begin_initialization();
    }

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute_module(&loaded)
        .expect_err("in-progress module result should not be synthesized");

    assert!(matches!(error, VmError::ModuleInitializing(key) if key == loaded.key()));
    assert_eq!(
        vm.runtime()
            .module_instance_snapshot(&loaded)
            .expect("module instance should exist")
            .state,
        ModuleInitializationState::Initializing
    );
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
fn module_epochs_keep_independent_init_results_and_private_slots() {
    let mut runtime = Runtime::default();
    let first_loaded = runtime
        .load_module("epoch_visible.kgr", module_with_private_init_slot(1))
        .expect("first module epoch should load");
    let second_loaded = runtime
        .load_module("epoch_visible.kgr", module_with_private_init_slot(2))
        .expect("second module epoch should load");

    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute_module(&first_loaded)
            .expect("first epoch should initialize"),
        Value::I32(1)
    );
    assert_eq!(
        vm.execute_module(&second_loaded)
            .expect("second epoch should initialize"),
        Value::I32(2)
    );

    let first_report = vm
        .execute(&first_loaded, "main")
        .expect("old epoch should remain executable");
    let latest = vm
        .runtime()
        .modules()
        .latest("epoch_visible.kgr")
        .expect("latest module epoch should be visible");
    let latest_report = vm
        .execute(&latest, "main")
        .expect("latest epoch should execute");

    assert_eq!(first_report.epoch, first_loaded.epoch.0);
    assert_eq!(first_report.return_value, Value::I32(1));
    assert_eq!(latest.epoch, second_loaded.epoch);
    assert_eq!(latest_report.epoch, second_loaded.epoch.0);
    assert_eq!(latest_report.return_value, Value::I32(2));
    assert_eq!(
        vm.runtime()
            .module_instance_snapshot(&first_loaded)
            .expect("first epoch instance should exist")
            .init_result,
        Some(Value::I32(1))
    );
    assert_eq!(
        vm.runtime()
            .module_instance_snapshot(&second_loaded)
            .expect("second epoch instance should exist")
            .init_result,
        Some(Value::I32(2))
    );
}

#[test]
fn reload_preserves_active_old_epoch_while_new_calls_use_latest_epoch() {
    let mut runtime = Runtime::default();
    let first_loaded = runtime
        .load_module("hot_reload.kgr", reloadable_value_module(1))
        .expect("first module epoch should load");
    assert!(
        runtime
            .modules()
            .retain_epoch(first_loaded.key(), ModuleEpochRetention::ActiveCall)
    );

    let second_loaded = runtime
        .reload_module(&first_loaded, "hot_reload.kgr", reloadable_value_module(2))
        .expect("compatible reload should publish a new epoch");

    assert_eq!(
        runtime.modules().collect_unreachable_epochs(),
        Vec::new(),
        "old active-call epoch must remain reachable after reload"
    );

    let mut vm = Vm::new(runtime);
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
    assert_eq!(old_report.return_value, Value::I32(1));
    assert_eq!(latest_report.epoch, second_loaded.epoch.0);
    assert_eq!(latest_report.return_value, Value::I32(2));

    assert!(
        vm.runtime()
            .modules()
            .release_epoch(first_loaded.key(), ModuleEpochRetention::ActiveCall)
    );
    assert_eq!(
        vm.runtime().modules().collect_unreachable_epochs(),
        vec![first_loaded.key()]
    );
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
