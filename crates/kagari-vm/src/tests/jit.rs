use kagari_common::Span;
use kagari_ir::{
    bytecode::{
        BinaryOp, BytecodeInstruction, ConstantOperand, DebugPointId, FunctionRef,
        InstructionSourceSpan, LineTableEntry, Register, SafeDebugPoint, SafeDebugPointKind,
        UnaryOp,
    },
    module::ValueType,
};
use kagari_jit_cranelift::CraneliftBackend;
use kagari_runtime::{
    BackendCompileError, BackendFunctionInput, BackendId, BackendInvocationError, BackendTarget,
    CapabilitySet, CodegenBackend, DebugVisibilityPolicy, ExecutableDebugInfo,
    ExecutableDebugPoint, ExecutableEntryPoint, ExecutableFunctionArtifact, ExecutableSafepoint,
    ExecutableSafepointKind, ExecutableStackMap, LanguageProfile, Runtime, RuntimeConfig,
    SecurityContext, value::Value,
};

use crate::{DebugSession, JitExecutionStatus, Vm, tests::common};

#[derive(Debug)]
struct UnsupportedBackend {
    backend: BackendId,
    target: BackendTarget,
}

impl UnsupportedBackend {
    fn new() -> Self {
        Self {
            backend: BackendId::new("test-unsupported-jit"),
            target: BackendTarget::new("test-target", 64),
        }
    }
}

impl CodegenBackend for UnsupportedBackend {
    fn backend_id(&self) -> BackendId {
        self.backend.clone()
    }

    fn target(&self) -> BackendTarget {
        self.target.clone()
    }

    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<ExecutableFunctionArtifact, BackendCompileError> {
        Err(BackendCompileError::unsupported(format!(
            "test backend cannot compile `{}`",
            input.function.name
        )))
    }
}

#[derive(Debug)]
struct NativeBackend {
    backend: BackendId,
    target: BackendTarget,
    supports_debugging: bool,
    compile_count: usize,
}

impl NativeBackend {
    fn new() -> Self {
        Self {
            backend: BackendId::new("test-native-jit"),
            target: BackendTarget::new("test-target", 64),
            supports_debugging: false,
            compile_count: 0,
        }
    }

    fn with_debug_metadata() -> Self {
        Self {
            backend: BackendId::new("test-native-jit"),
            target: BackendTarget::new("test-target", 64),
            supports_debugging: true,
            compile_count: 0,
        }
    }

    fn compile_count(&self) -> usize {
        self.compile_count
    }
}

impl CodegenBackend for NativeBackend {
    fn backend_id(&self) -> BackendId {
        self.backend.clone()
    }

    fn target(&self) -> BackendTarget {
        self.target.clone()
    }

    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<ExecutableFunctionArtifact, BackendCompileError> {
        self.compile_count += 1;
        let mut artifact =
            ExecutableFunctionArtifact::new(self.backend_id(), self.target(), input.function_ref());
        artifact.entry =
            ExecutableEntryPoint::Symbol(format!("{}::{}", input.module_name, input.function.name));
        artifact.safepoints.push(ExecutableSafepoint {
            instruction_offset: 0,
            kind: ExecutableSafepointKind::RuntimeHelperCall {
                helper: "test.consume_instruction_step".to_owned(),
            },
            stack_map: ExecutableStackMap::empty(),
        });
        if self.supports_debugging {
            artifact.debug = ExecutableDebugInfo {
                has_line_tables: true,
                has_source_spans: true,
                has_live_value_locations: true,
                has_safe_debug_callbacks: true,
                safe_debug_points: input
                    .function
                    .metadata
                    .debug
                    .safe_debug_points
                    .iter()
                    .map(|point| ExecutableDebugPoint {
                        instruction_offset: point.instruction_offset,
                        debug_point: point.id,
                    })
                    .collect(),
            };
        }
        Ok(artifact)
    }

    fn invoke_function(
        &self,
        artifact: &ExecutableFunctionArtifact,
        runtime: &Runtime,
    ) -> Result<Value, BackendInvocationError> {
        assert_eq!(artifact.function, FunctionRef::new(0));
        runtime
            .consume_instruction_step()
            .map_err(|error| BackendInvocationError::runtime_failure(error.to_string()))?;
        Ok(Value::I32(11))
    }
}

#[test]
fn jit_unsupported_compile_falls_back_to_interpreter_with_diagnostics() {
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
    let mut backend = UnsupportedBackend::new();

    let report = vm
        .execute_with_backend(&loaded, "main", &mut backend)
        .expect("unsupported JIT compilation should fall back");

    assert_eq!(report.return_value, Value::I32(7));
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.backend, BackendId::new("test-unsupported-jit"));
    assert_eq!(jit.function, FunctionRef::new(0));
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert!(jit.diagnostics[0].message.contains("cannot compile"));
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
        common::load_bytecode_module_with_runtime(jit_runtime(), "jit_stdlib_fallback", module);
    let mut vm = Vm::new(runtime);
    let mut backend = UnsupportedBackend::new();

    let report = vm
        .execute_with_backend(&loaded, "main", &mut backend)
        .expect("unsupported JIT compilation should fall back");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![Value::I64(3), Value::I64(2), Value::I32(7)])
    );
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
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
fn jit_native_execution_reports_registered_artifact() {
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
    let mut backend = NativeBackend::new();

    let report = vm
        .execute_with_backend(&loaded, "main", &mut backend)
        .expect("native JIT execution should succeed");

    assert_eq!(report.return_value, Value::I32(11));
    let jit = report.jit.expect("JIT execution should be reported");
    assert_eq!(jit.backend, BackendId::new("test-native-jit"));
    assert_eq!(jit.status, JitExecutionStatus::Native);
    assert!(jit.artifact.is_some());
    assert!(jit.diagnostics.is_empty());
    assert_eq!(vm.runtime().resources().counters().instruction_steps, 1);
}

#[test]
fn jit_policy_disablement_falls_back_before_backend_compile() {
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
    let (runtime, loaded) = common::load_bytecode_module("jit_policy_disabled", module);
    let mut vm = Vm::new(runtime);
    let mut backend = NativeBackend::new();

    let report = vm
        .execute_with_backend(&loaded, "main", &mut backend)
        .expect("disabled JIT policy should fall back to the interpreter");

    assert_eq!(report.return_value, Value::I32(7));
    assert_eq!(backend.compile_count(), 0);
    let jit = report.jit.expect("policy fallback should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert!(jit.diagnostics[0].message.contains("runtime policy"));
    assert!(jit.diagnostics[0].message.contains("jit"));
}

#[test]
fn jit_equivalence_matches_interpreter_for_compiled_scalar_functions() {
    let cases = vec![
        (
            "jit_eq_arithmetic",
            common::test_function_module(
                "main",
                vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantOperand::I32(6),
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(1),
                        constant: ConstantOperand::I32(7),
                    },
                    BytecodeInstruction::Binary {
                        dst: Register::new(2),
                        op: BinaryOp::Mul,
                        lhs: Register::new(0),
                        rhs: Register::new(1),
                    },
                    BytecodeInstruction::Unary {
                        dst: Register::new(3),
                        op: UnaryOp::Neg,
                        operand: Register::new(2),
                    },
                    BytecodeInstruction::Return(Some(Register::new(3))),
                ],
                ValueType::I32,
                vec![
                    ValueType::I32,
                    ValueType::I32,
                    ValueType::I32,
                    ValueType::I32,
                ],
            ),
        ),
        (
            "jit_eq_comparison",
            common::test_function_module(
                "main",
                vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantOperand::I32(40),
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(1),
                        constant: ConstantOperand::I32(2),
                    },
                    BytecodeInstruction::Binary {
                        dst: Register::new(2),
                        op: BinaryOp::Add,
                        lhs: Register::new(0),
                        rhs: Register::new(1),
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(3),
                        constant: ConstantOperand::I32(42),
                    },
                    BytecodeInstruction::Binary {
                        dst: Register::new(4),
                        op: BinaryOp::Eq,
                        lhs: Register::new(2),
                        rhs: Register::new(3),
                    },
                    BytecodeInstruction::Unary {
                        dst: Register::new(5),
                        op: UnaryOp::Not,
                        operand: Register::new(4),
                    },
                    BytecodeInstruction::Unary {
                        dst: Register::new(6),
                        op: UnaryOp::Not,
                        operand: Register::new(5),
                    },
                    BytecodeInstruction::Return(Some(Register::new(6))),
                ],
                ValueType::Bool,
                vec![
                    ValueType::I32,
                    ValueType::I32,
                    ValueType::I32,
                    ValueType::I32,
                    ValueType::Bool,
                    ValueType::Bool,
                    ValueType::Bool,
                ],
            ),
        ),
    ];

    for (module_name, module) in cases {
        let (runtime, loaded) = common::load_bytecode_module(module_name, module.clone());
        let mut vm = Vm::new(runtime);
        let interpreted = vm
            .execute(&loaded, "main")
            .expect("interpreter execution should succeed");
        let (runtime, loaded) =
            common::load_bytecode_module_with_runtime(jit_runtime(), module_name, module);
        let mut vm = Vm::new(runtime);
        let mut backend =
            CraneliftBackend::for_host().expect("host Cranelift target should initialize");

        let compiled = vm
            .execute_with_backend(&loaded, "main", &mut backend)
            .expect("eligible scalar bytecode should run through the JIT");

        assert_eq!(compiled.return_value, interpreted.return_value);
        let jit = compiled.jit.expect("JIT execution should be reported");
        assert_eq!(jit.status, JitExecutionStatus::Native);
        assert!(jit.artifact.is_some());
        assert!(jit.diagnostics.is_empty());
    }
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
    let mut backend = NativeBackend::new();

    let report = vm
        .execute_with_backend(&loaded, "main", &mut backend)
        .expect("debugger should force interpreter fallback when JIT metadata is missing");

    assert_eq!(report.return_value, Value::I32(7));
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
    assert!(jit.diagnostics[0].message.contains("while debugging"));
    assert!(
        jit.diagnostics[0]
            .message
            .contains("safe debug point callbacks")
    );
}

#[test]
fn jit_debug_session_allows_native_when_safe_debug_metadata_is_complete() {
    let module = debug_test_module(7);
    let runtime = debug_runtime("jit_debug_native");
    let session = DebugSession::new(&runtime).expect("debug session should attach");
    let (runtime, loaded) =
        common::load_bytecode_module_with_runtime(runtime, "jit_debug_native", module);
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session)
        .expect("debug session should attach to VM");
    let mut backend = NativeBackend::with_debug_metadata();

    let report = vm
        .execute_with_backend(&loaded, "main", &mut backend)
        .expect("complete debug metadata should allow native JIT execution");

    assert_eq!(report.return_value, Value::I32(11));
    let jit = report.jit.expect("JIT execution should be reported");
    assert_eq!(jit.status, JitExecutionStatus::Native);
    assert!(jit.artifact.is_some());
    assert!(jit.diagnostics.is_empty());
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

fn debug_test_module(value: i32) -> kagari_ir::bytecode::BytecodeModule {
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
