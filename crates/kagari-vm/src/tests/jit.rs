use kagari_abi::{
    ids::{DebugPointId, FunctionRef},
    native::BackendId,
    representation::ValueType,
};
use kagari_bytecode::{
    BytecodeInstruction, ConstantOperand, InstructionSourceSpan, LineTableEntry, Register,
    SafeDebugPoint, SafeDebugPointKind,
};
use kagari_common::Span;
use kagari_runtime::{
    CapabilitySet, DebugVisibilityPolicy, LanguageProfile, Runtime, RuntimeConfig, SecurityContext,
    value::Value,
};

use crate::{
    DebugSession, JitExecutionStatus, PreparedNativeEntry, Vm,
    tests::{common, native_fixtures},
};

#[test]
fn source_artifact_and_jit_fallback_resolve_imports_to_registered_slots() {
    use kagari_bytecode::{ArtifactBuildOptions, ArtifactCompatibility, HostImportId, KbcArtifact};
    use kagari_common::host_interface::{HostFunctionDeclaration, HostValueType, standard_log};
    use kagari_runtime::{HostExposurePolicy, host::HostFunction};
    use std::sync::{Arc, Mutex};
    let bytecode = common::compile_test_bytecode(r#"fn main() -> i32 { print("linked"); 7 }"#);
    for artifact in [false, true] {
        for jit in [false, true] {
            let module = if artifact {
                let encoded =
                    KbcArtifact::from_program(bytecode.clone(), ArtifactBuildOptions::default())
                        .unwrap()
                        .to_bytes()
                        .unwrap();
                let decoded = KbcArtifact::from_bytes(&encoded).unwrap();
                decoded
                    .validate_for_loader(&ArtifactCompatibility::default())
                    .unwrap();
                decoded.program
            } else {
                bytecode.clone()
            };
            let mut runtime = Runtime::new(RuntimeConfig {
                security: SecurityContext {
                    profile: LanguageProfile {
                        allow_host_calls: true,
                        allow_jit: true,
                        ..Default::default()
                    },
                    capabilities: CapabilitySet {
                        host_calls: true,
                        jit: true,
                        ..Default::default()
                    },
                },
                host_exposure: HostExposurePolicy {
                    allowed_host_functions: vec!["host.log".into()],
                    ..Default::default()
                },
                ..Default::default()
            });
            runtime
                .register_host_function(HostFunction::new(
                    HostFunctionDeclaration::new("host.unrelated", vec![], HostValueType::Unit),
                    |_, _| panic!("import 0 must not invoke registry slot 0"),
                ))
                .unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let called = calls.clone();
            let binding = runtime
                .register_host_function(HostFunction::new(standard_log(), move |_, args| {
                    called.lock().unwrap().push(args.to_vec());
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime.load_program("linked", module).unwrap();
            assert_eq!(loaded.host_binding(HostImportId::new(0)), Some(binding));
            assert_eq!(binding.index(), 1);
            let mut vm = Vm::new(runtime);
            let report = if jit {
                vm.execute_prepared(&loaded, "main", &native_fixtures::unsupported())
                    .unwrap()
            } else {
                vm.execute(&loaded, "main").unwrap()
            };
            assert_eq!(report.return_value, Value::I32(7));
            assert_eq!(
                *calls.lock().unwrap(),
                vec![vec![Value::Str("linked".into())]]
            );
        }
    }
}

#[test]
fn jit_unsupported_preparation_falls_back_to_interpreter_with_diagnostics() {
    let module = common::test_function_module(
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
    );
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(jit_runtime(), "jit_fallback", module);
    let mut vm = Vm::new(runtime);
    let prepared = native_fixtures::unsupported();

    let report = vm
        .execute_prepared(&loaded, "main", &prepared)
        .expect("unsupported JIT compilation should fall back");

    assert_eq!(report.return_value, Value::I32(7));
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.backend, BackendId::new("test-unsupported-jit"));
    assert_eq!(jit.function, FunctionRef::new(0));
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert!(jit.diagnostics[0].contains("cannot compile"));
}

#[test]
fn jit_fallback_executes_standard_intrinsics_deterministically() {
    let module = common::compile_test_bytecode(
        r#"
fn main() -> (usize, usize, i32) {
    val values = [1, 2];
    values.push(3);
    (values.len(), "ok".len_chars(), std::math::max(4, 7))
}

"#,
    );
    let (runtime, loaded) =
        common::load_bytecode_program_with_runtime(jit_runtime(), "jit_stdlib_fallback", module);
    let mut vm = Vm::new(runtime);
    let prepared = native_fixtures::unsupported();

    let report = vm
        .execute_prepared(&loaded, "main", &prepared)
        .expect("unsupported JIT compilation should fall back");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![Value::U64(3), Value::U64(2), Value::I32(7)])
    );
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
}

#[test]
fn remainder_uses_interpreter_fallback_with_identical_result() {
    let module = common::compile_test_bytecode("fn main() -> i32 { 42 % 5 }");
    let (runtime, loaded) = common::load_bytecode_program("jit_remainder", module.clone());
    let mut vm = Vm::new(runtime);
    let interpreted = vm.execute(&loaded, "main").unwrap().return_value;
    let (runtime, loaded) =
        common::load_bytecode_program_with_runtime(jit_runtime(), "jit_remainder", module);
    let mut vm = Vm::new(runtime);
    let prepared = native_fixtures::unsupported();
    let report = vm.execute_prepared(&loaded, "main", &prepared).unwrap();
    assert_eq!(report.return_value, interpreted);
    assert_eq!(report.return_value, Value::I32(2));
    assert_eq!(
        report.jit.unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
}

#[test]
fn closures_use_interpreter_fallback_with_identical_result() {
    let module = common::compile_test_bytecode(
        r#"
fn main() -> i32 {
    var count = 40;
    val next = || { count = count + 1; count };
    next();
    next()
}
"#,
    );
    let (runtime, loaded) = common::load_bytecode_program("jit_closure", module.clone());
    let mut vm = Vm::new(runtime);
    let interpreted = vm.execute(&loaded, "main").unwrap().return_value;
    let (runtime, loaded) =
        common::load_bytecode_program_with_runtime(jit_runtime(), "jit_closure", module);
    let mut vm = Vm::new(runtime);
    let prepared = native_fixtures::unsupported();
    let report = vm.execute_prepared(&loaded, "main", &prepared).unwrap();
    assert_eq!(report.return_value, Value::I32(42));
    assert_eq!(report.return_value, interpreted);
    assert_eq!(
        report.jit.unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
}

#[test]
fn ordinary_interpreter_execution_has_no_jit_report() {
    let module = common::test_function_module(
        "main",
        vec![BytecodeInstruction::Return(None)],
        ValueType::Unit,
        Vec::new(),
    );
    let (runtime, loaded) = common::load_bytecode_module("interpreter_only", module);
    let mut vm = Vm::new(runtime);

    let report = vm
        .execute(&loaded, "main")
        .expect("interpreter execution should succeed");

    assert_eq!(report.return_value, Value::Unit);
    assert!(report.jit.is_none());
}

#[test]
fn jit_native_execution_reports_installed_artifact() {
    let module = common::test_function_module(
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
    );
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(jit_runtime(), "jit_native", module);
    let mut vm = Vm::new(runtime);
    let prepared = PreparedNativeEntry::Native(native_fixtures::install_i32::<7>(
        vm.runtime(),
        &loaded,
        false,
    ));

    let report = vm
        .execute_prepared(&loaded, "main", &prepared)
        .expect("native JIT execution should succeed");

    assert_eq!(report.return_value, Value::I32(7));
    let jit = report.jit.expect("JIT execution should be reported");
    assert_eq!(jit.backend, BackendId::new("test-native-jit"));
    assert_eq!(jit.status, JitExecutionStatus::Native);
    assert!(jit.artifact.is_some());
    assert!(jit.diagnostics.is_empty());
    assert_eq!(vm.runtime().resources().counters().instruction_steps, 2);
}

#[test]
fn jit_policy_disablement_falls_back_before_native_entry() {
    let module = common::test_function_module(
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
    );
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(jit_runtime(), "jit_policy_disabled", module);
    let mut vm = Vm::new(runtime);
    let prepared = PreparedNativeEntry::Native(native_fixtures::install_i32::<7>(
        vm.runtime(),
        &loaded,
        false,
    ));

    vm.runtime_mut().set_security_context(Default::default());

    let report = vm
        .execute_prepared(&loaded, "main", &prepared)
        .expect("disabled JIT policy should fall back to the interpreter");

    assert_eq!(report.return_value, Value::I32(7));
    let jit = report.jit.expect("policy fallback should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert!(jit.diagnostics[0].contains("runtime policy"));
    assert!(jit.diagnostics[0].contains("jit"));
}

#[test]
fn jit_debug_session_falls_back_without_safe_debug_metadata() {
    let module = debug_test_module(7);
    let runtime = debug_runtime("jit_debug_fallback");
    let session = DebugSession::new(&runtime).expect("debug session should attach");
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(runtime, "jit_debug_fallback", module);
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug session should attach to VM");
    let prepared = PreparedNativeEntry::Native(native_fixtures::install_i32::<7>(
        vm.runtime(),
        &loaded,
        false,
    ));

    let report = vm
        .execute_prepared(&loaded, "main", &prepared)
        .expect("debugger should force interpreter fallback when JIT metadata is missing");

    assert_eq!(report.return_value, Value::I32(7));
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert_eq!(
        jit.diagnostics,
        ["native invocation with an execution observer is unsupported"]
    );
}

#[test]
fn jit_debug_session_requires_callbacks_even_when_metadata_is_complete() {
    let module = debug_test_module(7);
    let runtime = debug_runtime("jit_debug_native");
    let session = DebugSession::new(&runtime).expect("debug session should attach");
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(runtime, "jit_debug_native", module);
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug session should attach to VM");
    let prepared = PreparedNativeEntry::Native(native_fixtures::install_i32::<7>(
        vm.runtime(),
        &loaded,
        true,
    ));

    let report = vm
        .execute_prepared(&loaded, "main", &prepared)
        .expect("metadata alone cannot supply native debug callbacks");

    assert_eq!(report.return_value, Value::I32(7));
    let jit = report.jit.expect("JIT execution should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert_eq!(
        jit.diagnostics,
        ["native invocation with an execution observer is unsupported"]
    );
}

fn debug_runtime(module_name: &str) -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                jit: true,
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

fn jit_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    })
}

fn debug_test_module(value: i32) -> kagari_bytecode::BytecodeModule {
    let mut module = common::test_function_module(
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
    );
    let span = Span::new(0, 4);
    let function = &mut module.functions[0];
    function.metadata.debug.source_spans = vec![InstructionSourceSpan {
        instruction_offset: 0,
        span,
    }];
    function.metadata.debug.line_table = vec![LineTableEntry {
        instruction_offset: 0,
        source_offset: 0,
        line: Some(1),
        column: Some(1),
    }];
    function.metadata.debug.safe_debug_points = vec![SafeDebugPoint {
        id: DebugPointId::new(0),
        instruction_offset: 0,
        span,
        kind: SafeDebugPointKind::FunctionEntry,
    }];
    module
}
