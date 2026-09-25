use std::sync::{Arc, Mutex};

use kagari_common::Span;
use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeInstruction, BytecodeModule, BytecodeModuleSlot, CallTarget,
    ConstantOperand, DebugPointId, FunctionMetadata, FunctionRecord, FunctionRef,
    InstructionSourceSpan, ModuleSlot, Register, RuntimeHelper, SafeDebugPoint, SafeDebugPointKind,
};
use kagari_ir::module::ValueType;
use kagari_runtime::host::{HostFunction, HostFunctionDeclaration};
use kagari_runtime::value::{StructValueField, Value};
use kagari_runtime::{
    CapabilitySet, DebugVisibilityPolicy, LanguageProfile, ModuleEpochRetention,
    ModuleInitializationState, ResourcePolicy, Runtime, RuntimeConfig, RuntimeErrorKind,
    SecurityContext,
};

use crate::tests::common::{compile_test_bytecode, load_test_module};
use crate::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint, Vm, VmError};

#[test]
fn foreign_loaded_module_is_rejected_before_initialization() {
    let bytecode = compile_test_bytecode("fn main() -> i32 { 7 }");
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    let foreign = first
        .load_program(
            "same",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode.clone()],
            },
        )
        .unwrap();
    let local = second
        .load_program(
            "same",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .unwrap();
    assert_eq!(foreign.key(), local.key());
    let mut vm = Vm::new(second);
    assert!(
        matches!(vm.execute(&foreign, "main"), Err(VmError::RuntimeError(ref error)) if error.kind() == RuntimeErrorKind::ModuleValidation)
    );
    assert_eq!(
        vm.runtime().module_instance_snapshot(&local).unwrap().state,
        ModuleInitializationState::Uninitialized
    );
    assert_eq!(
        vm.execute(&local, "main").unwrap().return_value,
        Value::I32(7)
    );
}

fn test_function(
    id: usize,
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
) -> BytecodeFunction {
    let metadata = FunctionMetadata {
        return_type,
        roots: kagari_ir::bytecode::RootSlotLayout::from_types(&[], &registers),
        registers,
        ..FunctionMetadata::default()
    };
    BytecodeFunction {
        id: FunctionRef::new(id),
        identity: None,
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
            identity: function.identity.clone(),
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

fn host_call_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: kagari_runtime::SecurityContext {
            profile: kagari_runtime::LanguageProfile {
                allow_host_calls: true,
                ..kagari_runtime::LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: kagari_runtime::HostExposurePolicy {
            allow_host_functions: true,
            ..kagari_runtime::HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    })
}

fn debug_runtime(module_name: &str) -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities: debug_capabilities(),
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec![module_name.to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    })
}

fn debug_capabilities() -> CapabilitySet {
    CapabilitySet {
        debug_attach: true,
        debug_breakpoints: true,
        debug_pause: true,
        debug_stack_inspection: true,
        debug_value_inspection: true,
        debug_watch_evaluation: true,
        ..CapabilitySet::default()
    }
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
        .load_program(
            "limited.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
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
fn reports_runtime_allocation_unit_limit() {
    let bytecode = compile_test_bytecode("fn main() -> i32 { val values = [1, 2]; 0 }");
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_allocation_units: Some(2),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "allocation_limited.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("array allocation should exceed allocation unit limit");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref err)
            if err.kind() == RuntimeErrorKind::ResourceLimitExceeded
                && err.message().contains("allocation units")
    ));
    assert_eq!(vm.runtime().resources().counters().allocation_units, 0);
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
        .load_program(
            "unverified.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
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
    let mut runtime = Runtime::default();
    for (name, bytecode) in [
        ("register_call.kbc", register_call),
        ("dynamic_call.kbc", dynamic_call),
    ] {
        let error = runtime
            .load_program(
                name,
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
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
fn unsupported_dynamic_invocation_is_rejected_even_with_capability() {
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
        security: kagari_runtime::SecurityContext {
            profile: kagari_runtime::LanguageProfile {
                allow_reflection: true,
                ..kagari_runtime::LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                dynamic_invocation: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    });
    let error = runtime
        .load_program(
            "dynamic_call_capable.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![dynamic_call],
            },
        )
        .expect_err("capability cannot authorize an unimplemented call form");
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(runtime.modules().loaded_count(), 0);
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
        .load_program(
            "call_depth.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
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
    let mut runtime = host_call_runtime();
    let loaded = runtime
        .load_program(
            "trap.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![verified_module(
                    None,
                    vec![test_function(
                        0,
                        "main",
                        vec![BytecodeInstruction::Unreachable],
                        ValueType::Unit,
                        vec![],
                    )],
                )],
            },
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
    let mut runtime = debug_runtime("debug.kgr");
    let loaded = runtime
        .load_program(
            "debug.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![compile_test_bytecode(source)],
            },
        )
        .expect("debug module should load");
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    let breakpoint_id = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug.kgr",
            source
                .find("value +")
                .expect("source should contain tail expr"),
        ))
        .expect("breakpoint should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
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
            .evaluate_watch(
                vm.runtime(),
                frame.id,
                &DebugWatch::Binding("value".to_owned()),
            )
            .expect("watch should read live local"),
        Value::I32(3)
    );
}

#[test]
fn debug_session_supports_step_into_and_trap_pause_events() {
    let mut runtime = debug_runtime("debug_trap.kbc");
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
        .load_program(
            "debug_trap.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![verified_module(None, vec![main])],
            },
        )
        .expect("trap module should load");
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    session.step_into().expect("step should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let error = vm
        .execute(&loaded, "main")
        .expect_err("unreachable should trap");

    assert!(matches!(error, VmError::Trap("unreachable")));
    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
    let pauses = debug.pauses();
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
fn debugger_attachment_requires_runtime_capability() {
    let runtime = Runtime::default();
    let error = DebugSession::new(&runtime).expect_err("default profile denies debugger attach");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("debug_attach")
    ));
}

#[test]
fn debugger_breakpoints_require_capability_and_visible_module() {
    let mut capabilities = debug_capabilities();
    capabilities.debug_breakpoints = false;
    let runtime_without_breakpoints = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities,
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec!["debug.kgr".to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let mut session =
        DebugSession::new(&runtime_without_breakpoints).expect("attach should be allowed");
    let error = session
        .add_breakpoint(SourceBreakpoint::at_source_offset("debug.kgr", 0))
        .expect_err("breakpoints should require debugger capability");
    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("debug_breakpoints")
    ));

    let runtime_with_hidden_module = debug_runtime("visible.kgr");
    let mut session =
        DebugSession::new(&runtime_with_hidden_module).expect("attach should be allowed");
    let error = session
        .add_breakpoint(SourceBreakpoint::at_source_offset("hidden.kgr", 0))
        .expect_err("hidden modules should not accept breakpoints");
    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("debug module `hidden.kgr`")
    ));
}

#[test]
fn debugger_pause_control_is_separate_from_breakpoint_capability() {
    let mut capabilities = debug_capabilities();
    capabilities.debug_breakpoints = false;
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities,
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec!["debug_step_only.kgr".to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "debug_step_only.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![compile_test_bytecode(
                    "fn main() -> i32 { val value = 1; value }",
                )],
            },
        )
        .expect("debug module should load");
    let mut session = DebugSession::new(&runtime).expect("attach should be allowed");
    session
        .step_into()
        .expect("pause control should be allowed");

    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let report = vm
        .execute(&loaded, "main")
        .expect("step-only debugger should execute without breakpoint capability");

    assert_eq!(report.return_value, Value::I32(1));
    assert!(
        vm.debug_session()
            .expect("debug session should be attached")
            .pauses()
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Step)
    );
}

#[test]
fn debugger_watch_evaluation_requires_separate_capability() {
    let source = r#"
fn main() -> i32 {
    val value = 3;
    value + 4
}
"#;
    let mut capabilities = debug_capabilities();
    capabilities.debug_watch_evaluation = false;
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities,
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec!["debug_watch.kgr".to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "debug_watch.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![compile_test_bytecode(source)],
            },
        )
        .expect("debug module should load");
    let mut session = DebugSession::new(&runtime).expect("debug attach should be allowed");
    let breakpoint = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug_watch.kgr",
            source
                .find("value +")
                .expect("source should contain tail expr"),
        ))
        .expect("breakpoints should be allowed");
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    vm.execute(&loaded, "main")
        .expect("debugged function should execute");

    let pause = vm
        .debug_session()
        .expect("debug session should be attached")
        .pauses()
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(breakpoint))
        .expect("breakpoint should pause")
        .clone();
    let frame = pause.top_frame().expect("pause should expose a frame");
    let error = pause
        .evaluate_watch(
            vm.runtime(),
            frame.id,
            &DebugWatch::Binding("value".to_owned()),
        )
        .expect_err("watch evaluation should require its own capability");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("debug_watch_evaluation")
    ));
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
    let mut runtime = host_call_runtime();
    let loaded = runtime
        .load_program(
            "initializing.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![module_with_private_init_slot(1)],
            },
        )
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
    let mut metadata = HostFunctionDeclaration::new(
        "host.secure",
        vec![],
        kagari_common::host_interface::HostValueType::I32,
    );
    metadata.capability_requirements = CapabilitySet {
        fs_read: true,
        ..CapabilitySet::default()
    };

    let mut runtime = host_call_runtime();
    runtime
        .register_host_function(HostFunction::new(metadata.clone(), move |_, _| {
            *calls_for_host
                .lock()
                .expect("host call counter should lock") += 1;
            Ok(Value::I32(1))
        }))
        .expect("host function should register");
    let loaded = runtime
        .load_program(
            "host_capability.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    verified_module(
                        None,
                        vec![test_function(
                            0,
                            "main",
                            vec![
                                BytecodeInstruction::Call {
                                    dst: Some(Register::new(0)),
                                    callee: CallTarget::HostFunction(
                                        kagari_ir::bytecode::HostImportId::new(0),
                                    ),
                                    args: vec![],
                                },
                                BytecodeInstruction::Return(Some(Register::new(0))),
                            ],
                            ValueType::I32,
                            vec![ValueType::I32],
                        )],
                    ),
                    vec![metadata.clone()],
                )],
            },
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
    let mut metadata = HostFunctionDeclaration::new(
        "host.costly",
        vec![],
        kagari_common::host_interface::HostValueType::I32,
    );
    metadata.resource_cost_hint = Some(2);

    let mut runtime = Runtime::new(RuntimeConfig {
        security: kagari_runtime::SecurityContext {
            profile: kagari_runtime::LanguageProfile {
                allow_host_calls: true,
                ..kagari_runtime::LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        resources: ResourcePolicy {
            max_instruction_steps: Some(2),
            ..ResourcePolicy::default()
        },
        host_exposure: kagari_runtime::HostExposurePolicy {
            allowed_host_functions: vec!["host.costly".to_owned()],
            ..kagari_runtime::HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    runtime
        .register_host_function(HostFunction::new(metadata.clone(), move |_, _| {
            *calls_for_host
                .lock()
                .expect("host call counter should lock") += 1;
            Ok(Value::I32(1))
        }))
        .expect("host function should register");
    let loaded = runtime
        .load_program(
            "host_cost.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    verified_module(
                        None,
                        vec![test_function(
                            0,
                            "main",
                            vec![
                                BytecodeInstruction::Call {
                                    dst: Some(Register::new(0)),
                                    callee: CallTarget::HostFunction(
                                        kagari_ir::bytecode::HostImportId::new(0),
                                    ),
                                    args: vec![],
                                },
                                BytecodeInstruction::Return(Some(Register::new(0))),
                            ],
                            ValueType::I32,
                            vec![ValueType::I32],
                        )],
                    ),
                    vec![metadata.clone()],
                )],
            },
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
fn host_runtime_helpers_enforce_host_call_resource_limit_before_invocation() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = Arc::clone(&calls);

    let mut runtime = Runtime::new(RuntimeConfig {
        security: kagari_runtime::SecurityContext {
            profile: kagari_runtime::LanguageProfile {
                allow_host_calls: true,
                ..kagari_runtime::LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        resources: ResourcePolicy {
            max_host_calls: Some(0),
            ..ResourcePolicy::default()
        },
        host_exposure: kagari_runtime::HostExposurePolicy {
            allowed_host_functions: vec!["host.limited".to_owned()],
            ..kagari_runtime::HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.limited",
                vec![],
                kagari_common::host_interface::HostValueType::I32,
            ),
            move |_, _| {
                *calls_for_host
                    .lock()
                    .expect("host call counter should lock") += 1;
                Ok(Value::I32(1))
            },
        ))
        .expect("host function should register");
    let loaded = runtime
        .load_program(
            "host_call_limit.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    verified_module(
                        None,
                        vec![test_function(
                            0,
                            "main",
                            vec![
                                BytecodeInstruction::Call {
                                    dst: Some(Register::new(0)),
                                    callee: CallTarget::HostFunction(
                                        kagari_ir::bytecode::HostImportId::new(0),
                                    ),
                                    args: vec![],
                                },
                                BytecodeInstruction::Return(Some(Register::new(0))),
                            ],
                            ValueType::I32,
                            vec![ValueType::I32],
                        )],
                    ),
                    vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                        "host.limited",
                        vec![],
                        kagari_common::host_interface::HostValueType::I32,
                    )],
                )],
            },
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("host helper should hit host call limit");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
                && error.message().contains("host calls")
    ));
    assert_eq!(*calls.lock().expect("host call counter should lock"), 0);
    assert_eq!(vm.runtime().resources().counters().host_calls, 0);
}

#[test]
fn executes_module_init_before_entry_only_once_per_module_epoch() {
    let init_count = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&init_count);

    let mut runtime = host_call_runtime();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.bump_init",
                vec![],
                kagari_common::host_interface::HostValueType::Unit,
            ),
            move |_, _| {
                let mut count = counter.lock().expect("counter lock should succeed");
                *count += 1;
                Ok(Value::Unit)
            },
        ))
        .expect("host function should register");

    let loaded = runtime
        .load_program(
            "module_init_once.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    verified_module(
                        Some(FunctionRef::new(0)),
                        vec![
                            test_function(
                                0,
                                "__module_init__",
                                vec![
                                    BytecodeInstruction::Call {
                                        dst: None,
                                        callee: CallTarget::HostFunction(
                                            kagari_ir::bytecode::HostImportId::new(0),
                                        ),
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
                    vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                        "host.bump_init",
                        vec![],
                        kagari_common::host_interface::HostValueType::Unit,
                    )],
                )],
            },
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

    let mut runtime = host_call_runtime();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.bump_init",
                vec![],
                kagari_common::host_interface::HostValueType::Unit,
            ),
            move |_, _| {
                let mut count = counter.lock().expect("counter lock should succeed");
                *count += 1;
                Ok(Value::Unit)
            },
        ))
        .expect("host function should register");

    let bytecode = crate::tests::common::with_host_imports(
        verified_module(
            Some(FunctionRef::new(0)),
            vec![test_function(
                0,
                "__module_init__",
                vec![
                    BytecodeInstruction::Call {
                        dst: None,
                        callee: CallTarget::HostFunction(kagari_ir::bytecode::HostImportId::new(0)),
                        args: vec![],
                    },
                    BytecodeInstruction::Return(None),
                ],
                ValueType::Unit,
                vec![],
            )],
        ),
        vec![kagari_common::host_interface::HostFunctionDeclaration::new(
            "host.bump_init",
            vec![],
            kagari_common::host_interface::HostValueType::Unit,
        )],
    );
    let first_loaded = runtime
        .load_program(
            "reloadable.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode.clone()],
            },
        )
        .expect("first module epoch should load");
    let second_loaded = runtime
        .load_program(
            "reloadable.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
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
    let mut runtime = host_call_runtime();
    let first_loaded = runtime
        .load_program(
            "epoch_visible.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![module_with_private_init_slot(1)],
            },
        )
        .expect("first module epoch should load");
    let second_loaded = runtime
        .load_program(
            "epoch_visible.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![module_with_private_init_slot(2)],
            },
        )
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
        .load_program(
            "hot_reload.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![reloadable_value_module(1)],
            },
        )
        .expect("first module epoch should load");
    assert!(
        runtime
            .modules()
            .retain_epoch(first_loaded.key(), ModuleEpochRetention::ActiveCall)
    );

    let second_loaded = runtime
        .stage_reload_program(
            &first_loaded,
            "hot_reload.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![reloadable_value_module(2)],
            },
        )
        .expect("compatible reload should publish a new epoch");
    let second_loaded = runtime.publish_staged_reload(second_loaded).unwrap();

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

    let mut runtime = host_call_runtime();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.fail_init",
                vec![],
                kagari_common::host_interface::HostValueType::Unit,
            ),
            move |_, _| {
                let mut count = counter.lock().expect("counter lock should succeed");
                *count += 1;
                Err(kagari_runtime::host::HostError::new("boom"))
            },
        ))
        .expect("host function should register");

    let loaded = runtime
        .load_program(
            "module_init_failed.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    verified_module(
                        Some(FunctionRef::new(0)),
                        vec![test_function(
                            0,
                            "__module_init__",
                            vec![
                                BytecodeInstruction::Call {
                                    dst: None,
                                    callee: CallTarget::HostFunction(
                                        kagari_ir::bytecode::HostImportId::new(0),
                                    ),
                                    args: vec![],
                                },
                                BytecodeInstruction::Return(None),
                            ],
                            ValueType::Unit,
                            vec![],
                        )],
                    ),
                    vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                        "host.fail_init",
                        vec![],
                        kagari_common::host_interface::HostValueType::Unit,
                    )],
                )],
            },
        )
        .expect("failed-init module should load");

    let mut vm = Vm::new(runtime);
    let first = vm
        .execute_module(&loaded)
        .expect_err("module init should fail");
    let second = vm
        .execute_module(&loaded)
        .expect_err("failed module should stay failed");

    assert!(matches!(
        first,
        VmError::RuntimeError(ref err)
            if err.kind() == RuntimeErrorKind::HostCallFailure && err.message().contains("boom")
    ));
    assert!(matches!(
        second,
        VmError::RuntimeError(ref err)
            if err.kind() == RuntimeErrorKind::HostCallFailure && err.message().contains("boom")
    ));
    assert_eq!(*init_count.lock().expect("counter lock should succeed"), 1);
}

#[test]
fn aggregate_field_instructions_reject_a_different_nominal_receiver() {
    // The low-level operand representation is HeapObject; the actual receiver
    // must still be checked against the field's nominal layout at execution.
    for write in [false, true] {
        let mut bytecode = compile_test_bytecode(
            "struct P { var x: i32 } struct Q { var x: i32 } fn main() -> i32 { val p = P { x: 1 }; p.x = 42; p.x }",
        );
        bytecode.module_slots.push(BytecodeModuleSlot {
            name: "observed".into(),
            ty: ValueType::HeapObject,
            mutable: true,
        });
        let function = &mut bytecode.functions[0];
        let (at, value) = function
            .instructions
            .iter()
            .enumerate()
            .find_map(|(at, instruction)| {
                if let BytecodeInstruction::MakeStruct { dst, .. } = instruction {
                    Some((at, *dst))
                } else {
                    None
                }
            })
            .unwrap();
        function.instructions.insert(
            at + 1,
            BytecodeInstruction::StoreModule {
                slot: ModuleSlot::new(0),
                src: value,
            },
        );
        let wrong = kagari_ir::bytecode::StructId::new(
            bytecode
                .structures
                .iter()
                .position(|layout| layout.name() == "Q")
                .unwrap(),
        );
        for instruction in bytecode
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
        let (runtime, loaded) = super::common::load_bytecode_module("wrong-receiver", bytecode);
        let mut vm = Vm::new(runtime);
        let error = vm.execute(&loaded, "main").unwrap_err();
        if write {
            assert!(matches!(error, VmError::RuntimeError(ref error)
                if error.kind() == kagari_runtime::RuntimeErrorKind::ScriptTrap
                    && error.message() == "struct layout mismatch"));
        } else {
            assert!(matches!(error, VmError::TypeMismatch(_)));
        }
        let instance = vm.runtime().module_instance_snapshot(&loaded).unwrap();
        let Value::Struct(object) = instance.module_slots[0] else {
            panic!("expected stored receiver")
        };
        let value =
            kagari_runtime::reflection::get_field(vm.runtime().gc(), &Value::Struct(object), "x")
                .unwrap();
        assert_eq!(value, Value::I32(if write { 1 } else { 42 }));
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    }
}

#[test]
fn concrete_interface_object_resolves_a_linked_method_slot() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn read<T: Tag>(x: T) -> i32 { x.tag() } fn main() -> i32 { read(7) }",
    );
    let table = &loaded.bytecode.interface_tables[0];
    let method = table.methods[0].method.clone();
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let resolved = runtime.resolve_interface_method(&boxed, &method).unwrap();
    assert_eq!(resolved.receiver(), &Value::I32(7));
    assert_eq!(resolved.implementation().key(), loaded.key());
    assert_eq!(resolved.function(), table.methods[0].function);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&boxed));
    assert!(
        runtime
            .resolve_interface_method(&Value::I32(7), &method)
            .is_err()
    );
    assert!(
        runtime
            .validate_interface_method_result(&resolved, &Value::Bool(true))
            .is_err()
    );

    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.invoke_interface_method(&boxed, &method, &[]).unwrap(),
        Value::I32(8)
    );
    assert!(
        vm.invoke_interface_method(&boxed, &method, &[Value::Bool(true)])
            .is_err()
    );
}

#[test]
fn interface_method_keeps_its_implementation_across_reload() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn read<T: Tag>(x: T) -> i32 { x.tag() } fn main() -> i32 { read(7) }";
    let first = compile_test_bytecode(source);
    let second = compile_test_bytecode(&source.replace("self + 1", "self + 2"));
    let mut runtime = Runtime::default();
    let program = |module| kagari_ir::bytecode::BytecodeProgram {
        root: kagari_ir::bytecode::ModuleRef::new(0),
        modules: vec![module],
    };
    let old = runtime
        .load_program("interface-reload", program(first))
        .unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method.clone();
    let old_value = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let old_root = runtime.root_value(old_value.clone()).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-reload", program(second))
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    let new_value = runtime.make_interface(&new, 0, Value::I32(7)).unwrap();
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.invoke_interface_method(&old_value, &method, &[])
            .unwrap(),
        Value::I32(8)
    );
    assert_eq!(
        vm.invoke_interface_method(&new_value, &method, &[])
            .unwrap(),
        Value::I32(9)
    );
    drop(old_root);
    vm.runtime().collect_garbage().unwrap();
    assert!(!vm.runtime().gc().validate_value(&old_value));
}

#[test]
fn interface_method_rejects_wrong_nominal_argument_before_execution() {
    let (runtime, loaded) = load_test_module(
        "struct A { val n: i32 } struct B { val n: i32 } trait Tag { fn read(self, x: A) -> i32; } impl Tag for i32 { fn read(self, x: A) -> i32 { x.n } } fn run<T: Tag>(x: T, a: A) -> i32 { x.read(a) } fn main() -> i32 { run(7, A { n: 1 }) } fn make_a() -> A { A { n: 5 } } fn make_b() -> B { B { n: 9 } }",
    );
    let method = loaded.bytecode.interface_tables[0].methods[0]
        .method
        .clone();
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let mut vm = Vm::new(runtime);
    let right = vm.execute(&loaded, "make_a").unwrap().return_value;
    assert_eq!(
        vm.invoke_interface_method(&boxed, &method, &[right])
            .unwrap(),
        Value::I32(5)
    );
    let wrong = vm.execute(&loaded, "make_b").unwrap().return_value;
    assert!(matches!(wrong, Value::Struct(_)));
    let error = vm
        .invoke_interface_method(&boxed, &method, &[wrong])
        .unwrap_err();
    assert!(
        matches!(error, VmError::RuntimeError(ref error) if error.message() == "interface method argument does not match its linked signature")
    );
}

fn interface_instruction_module() -> BytecodeModule {
    use kagari_common::identity::{
        DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
    };
    use kagari_ir::bytecode::{InterfaceTableRecord, InterfaceTableRef};
    use kagari_ir::module::{
        InterfaceTableAbi, PublicAbiItem, TraitAbi,
        abi::{AbiType, BuiltinType, NominalAbiType},
    };

    let identity = ModuleIdentity::single_file("interface-instruction.kgr");
    let declaration = |kind, name: &str| DefinitionId {
        module: identity.clone(),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    };
    let trait_id = declaration(DefinitionKind::Trait, "Tag");
    let impl_id = declaration(DefinitionKind::Impl, "");
    let mut module = verified_module(
        None,
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(7),
                },
                BytecodeInstruction::MakeInterface {
                    dst: Register::new(1),
                    value: Register::new(0),
                    implementation: InterfaceTableRef::new(0),
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
            ValueType::HeapObject,
            vec![ValueType::I32, ValueType::HeapObject],
        )],
    );
    module.identity = identity;
    module.public_items = vec![
        PublicAbiItem::Trait(TraitAbi {
            name: "Tag".into(),
            generic_params: vec![],
            bounds: vec![],
            methods: vec![],
        }),
        PublicAbiItem::InterfaceTable(InterfaceTableAbi {
            declaration: impl_id.clone(),
            name: String::new(),
            generic_params: vec![],
            bounds: vec![],
            trait_type: AbiType::Trait(NominalAbiType {
                declaration: trait_id,
                arguments: vec![],
            }),
            for_type: AbiType::Builtin(BuiltinType::I32),
            methods: vec![],
        }),
    ];
    module.interface_tables = vec![InterfaceTableRecord {
        declaration: impl_id,
        methods: vec![],
    }];
    module
}

#[test]
fn linked_interface_instruction_executes_and_rejects_invalid_slots() {
    use kagari_ir::bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, BytecodeProgram, InterfaceTableRef,
        KbcArtifact, ModuleRef, verify_module,
    };
    let module = interface_instruction_module();
    verify_module(&module).unwrap();

    let mut invalid = module.clone();
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Register::new(1),
        value: Register::new(0),
        implementation: InterfaceTableRef::new(1),
    };
    assert!(verify_module(&invalid).is_err());
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Register::new(1),
        value: Register::new(1),
        implementation: InterfaceTableRef::new(0),
    };
    assert!(verify_module(&invalid).is_err());

    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program("interface-instruction", decoded.program)
        .unwrap();
    let mut vm = Vm::new(runtime);
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    assert!(matches!(value, Value::Interface(_)));
    assert!(vm.runtime().gc().validate_value(&value));
}
