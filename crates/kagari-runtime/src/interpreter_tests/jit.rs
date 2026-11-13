use crate::Runtime;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::bytecode::reload_bytecode;
use crate::{RuntimeConfig, module::entry::ProgramEntry};
use crate::{
    debug::DebugSession,
    execution::{
        JitExecutionStatus,
        fixtures::FixtureExecution,
        native::{PreparedNativeEntry, PreparedNativeExecution},
    },
    interpreter_tests::{common, common::test_runtime, native_fixtures},
};
use kagari_abi::{native::BackendId, representation::ValueType};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, ConstantId, ConstantOperand, Slot},
    module::{
        BytecodeModule, InstructionSourceSpan, LineTableEntry, SafeDebugPoint, SafeDebugPointKind,
    },
};
use kagari_common::{identity::metadata::DefinitionMetadata, span::Span};
use kagari_contract::ids::{DebugPointId, FunctionRef};

#[test]
fn jit_unsupported_preparation_falls_back_to_interpreter_with_diagnostics() {
    let module = common::test_function_module(
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Slot::new(0),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::Return(Some(Slot::new(0))),
        ],
        ValueType::I32,
        vec![ValueType::I32],
        vec![ConstantOperand::I32(7)],
    );
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(jit_runtime(), "jit_fallback", module);
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let prepared = native_fixtures::unsupported();

    let report = vm
        .execute_fixture_prepared(&loaded, FunctionRef::new(0), &prepared)
        .expect("unsupported JIT compilation should fall back");

    assert_eq!(report.return_value.as_i32(), Some(7));
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.backend, BackendId::new("test-unsupported-jit"));
    assert_eq!(jit.function, FunctionRef::new(0));
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert!(jit.diagnostics[0].contains("cannot compile"));
}

#[test]
fn remainder_uses_interpreter_fallback_with_identical_result() {
    let module = common::compile_test_bytecode(include_str!(
        "../../tests/fixtures/execution/jit-remainder.kgr"
    ));
    let (runtime, loaded) = common::load_bytecode_program("jit_remainder", module.clone());
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let interpreted = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .as_i32();
    let (runtime, loaded) =
        common::load_bytecode_program_with_runtime(jit_runtime(), "jit_remainder", module);
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let prepared = native_fixtures::unsupported();
    let report = vm
        .execute_prepared_module(
            &loaded,
            ProgramEntry::Main,
            &prepared,
            vm.execution_options(),
        )
        .unwrap();
    assert_eq!(report.return_value.as_i32(), interpreted);
    assert_eq!(report.return_value.as_i32(), Some(2));
    assert_eq!(
        report.jit.unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
}

#[test]
fn closures_use_interpreter_fallback_with_identical_result() {
    let module = common::compile_test_bytecode(include_str!(
        "../../tests/fixtures/execution/jit-closure.kgr"
    ));
    let (runtime, loaded) = common::load_bytecode_program("jit_closure", module.clone());
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let interpreted = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .as_i32();
    let (runtime, loaded) =
        common::load_bytecode_program_with_runtime(jit_runtime(), "jit_closure", module);
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let prepared = native_fixtures::unsupported();
    let report = vm
        .execute_prepared_module(
            &loaded,
            ProgramEntry::Main,
            &prepared,
            vm.execution_options(),
        )
        .unwrap();
    assert_eq!(report.return_value.as_i32(), Some(42));
    assert_eq!(report.return_value.as_i32(), interpreted);
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
        vec![],
    );
    let (runtime, loaded) = common::load_bytecode_module("interpreter_only", module);
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;

    let report = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .expect("interpreter execution should succeed");

    let mut source = vm.host_scope(&[]).unwrap();
    assert!(source.import(&report.return_value).unwrap().is_unit());
    drop(source);
    assert!(report.jit.is_none());
}

#[test]
fn native_products_must_match_current_selection_before_entry_or_fallback() {
    let program_code = |value| {
        common::module_program(common::test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Return(Some(Slot::new(0))),
            ],
            ValueType::I32,
            vec![ValueType::I32],
            vec![ConstantOperand::I32(value)],
        ))
    };
    let mut runtime = jit_runtime();
    let program = load_bytecode(&mut runtime, "native-current", program_code(7)).unwrap();
    let old = runtime.select_program(&program).unwrap();
    let stale = native_fixtures::install_i32::<7>(&mut runtime, &old, false);
    let mut vm = runtime;
    reload_bytecode(&mut vm, &program, None, program_code(9)).unwrap();
    let current = vm.select_program(&program).unwrap();
    let prepared = PreparedNativeEntry::Native(stale.clone());
    for fallback_reason in [None, Some("host requests interpretation")] {
        assert!(
            vm.execute_fixture_prepared(
                &old,
                FunctionRef::new(0),
                PreparedNativeExecution {
                    prepared: &prepared,
                    fallback_reason,
                }
            )
            .is_err(),
            "a stale product cannot authorize entry or fallback"
        );
    }
    assert!(vm.invoke_native_function(&stale).is_err());
    let prepared =
        PreparedNativeEntry::Native(native_fixtures::install_i32::<9>(&mut vm, &current, false));
    let report = vm
        .execute_fixture_prepared(&old, FunctionRef::new(0), &prepared)
        .unwrap();
    assert_eq!(report.epoch, current.epoch.0);
    assert_eq!(report.return_value.as_i32(), Some(9));
    assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    let report = vm
        .execute_fixture_prepared(&old, FunctionRef::new(0), &native_fixtures::unsupported())
        .unwrap();
    assert_eq!(report.epoch, current.epoch.0);
    assert_eq!(report.return_value.as_i32(), Some(9));
    assert_eq!(
        report.jit.unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(vm.execution_root().is_none());
    assert!(!vm.is_quarantined());
}

#[test]
fn jit_native_execution_reports_installed_artifact() {
    let module = common::test_function_module(
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Slot::new(0),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::Return(Some(Slot::new(0))),
        ],
        ValueType::I32,
        vec![ValueType::I32],
        vec![ConstantOperand::I32(7)],
    );
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(jit_runtime(), "jit_native", module);
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let prepared =
        PreparedNativeEntry::Native(native_fixtures::install_i32::<7>(&mut vm, &loaded, false));

    let report = vm
        .execute_fixture_prepared(&loaded, FunctionRef::new(0), &prepared)
        .expect("native JIT execution should succeed");

    assert_eq!(report.return_value.as_i32(), Some(7));
    let jit = report.jit.expect("JIT execution should be reported");
    assert_eq!(jit.backend, BackendId::new("test-native-jit"));
    assert_eq!(jit.status, JitExecutionStatus::Native);
    assert!(jit.artifact.is_some());
    assert!(jit.diagnostics.is_empty());
}

#[test]
fn jit_debug_session_falls_back_without_safe_debug_metadata() {
    let module = debug_test_module(7);
    let runtime = debug_runtime("jit_debug_fallback");
    let session = DebugSession::new(&runtime).expect("debug session should attach");
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(runtime, "jit_debug_fallback", module);
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.attach_debug_session(session)
        .expect("debug session should attach to VM");
    let prepared =
        PreparedNativeEntry::Native(native_fixtures::install_i32::<7>(&mut vm, &loaded, false));

    let report = vm
        .execute_fixture_prepared(&loaded, FunctionRef::new(0), &prepared)
        .expect("debugger should force interpreter fallback when JIT metadata is missing");

    assert_eq!(report.return_value.as_i32(), Some(7));
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
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.attach_debug_session(session)
        .expect("debug session should attach to VM");
    let prepared =
        PreparedNativeEntry::Native(native_fixtures::install_i32::<7>(&mut vm, &loaded, true));

    let report = vm
        .execute_fixture_prepared(&loaded, FunctionRef::new(0), &prepared)
        .expect("metadata alone cannot supply native debug callbacks");

    assert_eq!(report.return_value.as_i32(), Some(7));
    let jit = report.jit.expect("JIT execution should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert_eq!(
        jit.diagnostics,
        ["native invocation with an execution observer is unsupported"]
    );
}

fn debug_runtime(_module_name: &str) -> Runtime {
    test_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn jit_runtime() -> Runtime {
    test_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn debug_test_module(value: i32) -> DefinitionMetadata<BytecodeModule> {
    let (definitions, mut module) = common::test_function_module(
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Slot::new(0),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::Return(Some(Slot::new(0))),
        ],
        ValueType::I32,
        vec![ValueType::I32],
        vec![ConstantOperand::I32(value)],
    )
    .into_parts();
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
    DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap()
}
