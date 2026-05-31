use kagari_ir::{
    bytecode::{BytecodeInstruction, ConstantOperand, FunctionRef, Register},
    module::ValueType,
};
use kagari_runtime::{
    BackendCompileError, BackendFunctionInput, BackendId, BackendInvocationError, BackendTarget,
    CodegenBackend, ExecutableEntryPoint, ExecutableFunctionArtifact, ExecutableSafepoint,
    ExecutableSafepointKind, ExecutableStackMap, Runtime, value::Value,
};

use crate::{JitExecutionStatus, Vm, tests::common};

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
}

impl NativeBackend {
    fn new() -> Self {
        Self {
            backend: BackendId::new("test-native-jit"),
            target: BackendTarget::new("test-target", 64),
        }
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
    let (runtime, loaded) = common::load_bytecode_module("jit_fallback", module);
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
    let (runtime, loaded) = common::load_bytecode_module("jit_native", module);
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
