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
    CapabilitySet, DebugVisibilityPolicy, LanguageProfile, ModuleEpochRetention, ResourcePolicy,
    Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext,
};

use crate::tests::common::{compile_test_bytecode, load_test_module};
use crate::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint, Vm, VmError};

#[test]
fn foreign_loaded_module_is_rejected_before_execution() {
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

fn verified_module(functions: Vec<BytecodeFunction>) -> BytecodeModule {
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
        constants,
        types,
        function_table,
        functions,
        ..BytecodeModule::default()
    }
}

fn module_with_mutable_slot(value: i32) -> BytecodeModule {
    let mut module = verified_module(vec![
        test_function(
            0,
            "init",
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
    ]);
    module.module_slots = vec![BytecodeModuleSlot {
        name: "private".to_owned(),
        ty: ValueType::I32,
        mutable: true,
    }];
    module
}

fn reloadable_value_module(value: i32) -> BytecodeModule {
    verified_module(vec![test_function(
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
    )])
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
    let before_store = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug.kgr",
            source
                .find("val value")
                .expect("source should contain binding"),
        ))
        .expect("initialization breakpoint should be allowed");
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
    let initial_pause = debug
        .pauses()
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(before_store))
        .expect("binding initializer should pause");
    assert!(
        initial_pause
            .top_frame()
            .unwrap()
            .bindings
            .iter()
            .all(|binding| binding.name != "value")
    );
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
fn debug_session_hides_inner_binding_after_lexical_scope() {
    let source = r#"
fn main() -> i32 {
    if true {
        val inner = 7;
        inner + 1;
    } else {
        0;
    };
    val after = 2;
    after
}
"#;
    let mut runtime = debug_runtime("lexical.kgr");
    let loaded = runtime
        .load_program(
            "lexical.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![compile_test_bytecode(source)],
            },
        )
        .unwrap();
    let mut session = DebugSession::new(&runtime).unwrap();
    let inside = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "lexical.kgr",
            source.find("inner +").unwrap(),
        ))
        .unwrap();
    let after = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "lexical.kgr",
            source.rfind("after").unwrap(),
        ))
        .unwrap();
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session).unwrap();
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(2)
    );
    let debug = vm.debug_session().unwrap();
    let pauses = debug.pauses();
    assert!(pauses.iter().any(|pause| {
        pause.reason == DebugPauseReason::Breakpoint(inside)
            && pause
                .top_frame()
                .unwrap()
                .bindings
                .iter()
                .any(|binding| binding.name == "inner")
    }));
    let after = pauses
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(after))
        .unwrap();
    assert!(
        after
            .top_frame()
            .unwrap()
            .bindings
            .iter()
            .all(|binding| binding.name != "inner")
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
                modules: vec![verified_module(vec![main])],
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
fn missing_linked_module_slot_quarantines_runtime_and_cleans_frames() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program(
            "module-slot-invariant",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![module_with_mutable_slot(7)],
            },
        )
        .unwrap();
    let mut vm = Vm::new(runtime);
    vm.execute(&loaded, "init").unwrap();
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(7)
    );
    vm.runtime()
        .module_instance_mut(&loaded)
        .unwrap()
        .module_slots
        .clear();

    let error = vm.execute(&loaded, "main").unwrap_err();
    assert!(
        matches!(error, VmError::RuntimeError(ref error) if error.kind() == kagari_runtime::RuntimeErrorKind::EngineFault)
    );
    assert!(vm.runtime().is_quarantined());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
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
                    verified_module(vec![test_function(
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
                    )]),
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
                    verified_module(vec![test_function(
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
                    )]),
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
                    verified_module(vec![test_function(
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
                    )]),
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
fn interface_method_slots_follow_trait_order_even_when_impl_order_differs() {
    use kagari_ir::module::{PublicAbiItem, abi::AbiType};
    let (runtime, loaded) = load_test_module(
        "trait Pair { fn first(self) -> i32; fn second(self) -> i32; } impl Pair for i32 { fn second(self) -> i32 { 2 } fn first(self) -> i32 { 1 } } fn main() -> i32 { 0 }",
    );
    let table = loaded
        .bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .unwrap();
    let AbiType::Trait(interface) = &table.trait_type else {
        panic!("expected trait interface")
    };
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let first = runtime
        .resolve_interface_method_slot(&boxed, interface, 0)
        .unwrap();
    let second = runtime
        .resolve_interface_method_slot(&boxed, interface, 1)
        .unwrap();
    let method = |name| {
        loaded.bytecode.interface_tables[0]
            .methods
            .iter()
            .find(|slot| slot.method.path.last().unwrap().name == name)
            .unwrap()
            .method
            .clone()
    };
    assert_eq!(
        first.function(),
        runtime
            .resolve_interface_method(&boxed, &method("first"))
            .unwrap()
            .function()
    );
    assert_eq!(
        second.function(),
        runtime
            .resolve_interface_method(&boxed, &method("second"))
            .unwrap()
            .function()
    );
    assert_ne!(first.function(), second.function());
    assert!(
        runtime
            .resolve_interface_method_slot(&boxed, interface, 2)
            .is_err()
    );
    let mut wrong = interface.clone();
    wrong.declaration.path.last_mut().unwrap().name = "Other".into();
    assert!(
        runtime
            .resolve_interface_method_slot(&boxed, &wrong, 0)
            .is_err()
    );
}

#[test]
fn source_call_boxes_a_concrete_argument_for_an_interface_parameter() {
    let (runtime, loaded) = load_test_module(
        "trait Tag {} impl Tag for i32 {} fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { accept(7) }",
    );
    assert!(
        loaded
            .bytecode
            .functions
            .iter()
            .flat_map(|function| &function.instructions)
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeInterface { .. }))
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn source_call_boxes_an_interface_with_methods() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { accept(7) }",
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn source_interface_method_call_dispatches_through_the_linked_slot() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn accept(value: Tag) -> i32 { value.tag() } fn main() -> i32 { accept(7) }",
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(8)
    );
}

#[test]
fn source_return_and_local_bindings_keep_the_boxed_interface_value() {
    let (runtime, loaded) = load_test_module(
        "trait Tag {} impl Tag for i32 {} fn make() -> Tag { 7 } fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { val value: Tag = make(); accept(value) }",
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
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
fn interface_frame_descendants_follow_the_receivers_pinned_program() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { helper() + self } } fn helper() -> i32 { 1 } fn main() -> i32 { 0 }";
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(
        &source.replace("fn helper() -> i32 { 1 }", "fn helper() -> i32 { 2 }"),
    );
    let program = |module| kagari_ir::bytecode::BytecodeProgram {
        root: kagari_ir::bytecode::ModuleRef::new(0),
        modules: vec![module],
    };
    let mut runtime = Runtime::default();
    let old = runtime
        .load_program("interface-frames", program(old_code))
        .unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method.clone();
    let boxed = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let _root = runtime.root_value(boxed.clone()).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-frames", program(new_code))
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    let resolved = runtime.resolve_interface_method(&boxed, &method).unwrap();
    let entry = new
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id;
    let helper = old
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "helper")
        .unwrap()
        .id;
    let stack = runtime.enter_execution_stack(&new).unwrap();
    stack.push(new.slot(), entry, &[], None).unwrap();
    let wrong = runtime.resolve_interface_method(&boxed, &method).unwrap();
    assert!(
        stack
            .push_interface_method(&runtime, wrong, &[Value::Bool(true)], None)
            .is_err()
    );
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack
        .push_interface_method(&runtime, resolved, &[Value::I32(7)], None)
        .unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), old.key());
    stack.push(old.slot(), helper, &[], None).unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), old.key());
    stack.pop().unwrap();
    stack.pop().unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack.pop().unwrap();
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn source_interface_dispatch_keeps_old_method_and_descendant_after_reload() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { helper() + self } } fn helper() -> i32 { 1 } fn read(value: Tag) -> i32 { value.tag() } fn main() -> i32 { read(7) }";
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(
        &source.replace("fn helper() -> i32 { 1 }", "fn helper() -> i32 { 2 }"),
    );
    let program = |module| kagari_ir::bytecode::BytecodeProgram {
        root: kagari_ir::bytecode::ModuleRef::new(0),
        modules: vec![module],
    };
    let mut runtime = Runtime::default();
    let old = runtime
        .load_program("interface-dispatch-reload", program(old_code))
        .unwrap();
    let old_value = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let old_root = runtime.root_value(old_value.clone()).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-dispatch-reload", program(new_code))
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    let new_value = runtime.make_interface(&new, 0, Value::I32(7)).unwrap();
    let read = new
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "read")
        .unwrap()
        .id;
    let mut old_call = crate::executor::Executor::new(&runtime, &new, read, &[old_value]).unwrap();
    assert_eq!(old_call.run().unwrap(), Value::I32(8));
    drop(old_call);
    let mut new_call = crate::executor::Executor::new(&runtime, &new, read, &[new_value]).unwrap();
    assert_eq!(new_call.run().unwrap(), Value::I32(9));
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(runtime.modules().collect_unreachable_epochs().is_empty());
    drop(new_call);
    drop(old_root);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.modules().collect_unreachable_epochs(),
        vec![old.key()]
    );
}

#[test]
fn trapped_interface_frame_releases_its_roots_and_call_budget() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self / 0 } } fn main() -> i32 { 42 }",
    );
    let method = loaded.bytecode.interface_tables[0].methods[0]
        .method
        .clone();
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let mut vm = Vm::new(runtime);
    assert!(vm.invoke_interface_method(&boxed, &method, &[]).is_err());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
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
    let mut module = verified_module(vec![test_function(
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
                module: kagari_ir::bytecode::ModuleRef::new(0),
                implementation: InterfaceTableRef::new(0),
            },
            BytecodeInstruction::Return(Some(Register::new(1))),
        ],
        ValueType::HeapObject,
        vec![ValueType::I32, ValueType::HeapObject],
    )]);
    module.identity = identity;
    module.public_items = vec![
        PublicAbiItem::Trait(TraitAbi {
            default_methods: Vec::new(),
            supertraits: Vec::new(),
            associated_types: Vec::new(),
            name: "Tag".into(),
            generic_params: vec![],
            bounds: vec![],
            methods: vec![],
        }),
        PublicAbiItem::InterfaceTable(Box::new(InterfaceTableAbi {
            host_bridge: false,
            declaration: impl_id.clone(),
            name: String::new(),
            generic_params: vec![],
            bounds: vec![],
            trait_type: AbiType::Trait(NominalAbiType {
                associated_types: Default::default(),
                declaration: trait_id,
                arguments: vec![],
            }),
            for_type: AbiType::Builtin(BuiltinType::I32),
            methods: vec![],
        })),
    ];
    module.interface_tables = vec![InterfaceTableRecord {
        arguments: Vec::new(),
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
        module: ModuleRef::new(0),
        implementation: InterfaceTableRef::new(1),
    };
    assert!(verify_module(&invalid).is_err());
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Register::new(1),
        value: Register::new(1),
        module: ModuleRef::new(0),
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

#[test]
fn interface_instruction_uses_a_reachable_dependency_table() {
    use kagari_common::identity::ModuleIdentity;
    use kagari_ir::bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, BytecodeProgram, InterfaceTableRef,
        KbcArtifact, ModuleRef, verify_program,
    };

    let dependency = interface_instruction_module();
    let mut consumer = verified_module(vec![test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Register::new(0),
                constant: ConstantOperand::I32(11),
            },
            BytecodeInstruction::MakeInterface {
                dst: Register::new(1),
                value: Register::new(0),
                module: ModuleRef::new(0),
                implementation: InterfaceTableRef::new(0),
            },
            BytecodeInstruction::Return(Some(Register::new(1))),
        ],
        ValueType::HeapObject,
        vec![ValueType::I32, ValueType::HeapObject],
    )]);
    consumer.identity = ModuleIdentity::single_file("interface-consumer.kgr");
    consumer.dependencies = vec![ModuleRef::new(0)];
    let program = BytecodeProgram {
        root: ModuleRef::new(1),
        modules: vec![dependency, consumer],
    };
    verify_program(&program).unwrap();
    let mut detached = program.clone();
    detached.modules[1].dependencies.clear();
    assert!(verify_program(&detached).is_err());

    let artifact = KbcArtifact::from_program(program, ArtifactBuildOptions::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();

    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program("interface-consumer", decoded.program)
        .unwrap();
    let dependency_key = loaded.member(ModuleRef::new(0)).unwrap().key();
    let mut vm = Vm::new(runtime);
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    assert!(matches!(value, Value::Interface(_)));
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(dependency_key)
            .runtime_values,
        1
    );
}
