use std::{fmt, mem, sync::Arc};

use cranelift_codegen::{
    Context,
    ir::{self, AbiParam, InstBuilder, MemFlags, condcodes::IntCC, types},
    isa::TargetIsa,
    settings::{self, Configurable},
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use kagari_ir::bytecode::{
    BinaryOp, BytecodeFunction, BytecodeInstruction, ConstantOperand, Register, UnaryOp,
    verify_module,
};
use kagari_runtime::{
    BackendCompileError, BackendDiagnostic, BackendDiagnosticKind, BackendFunctionInput, BackendId,
    BackendTarget, CodegenBackend, ExecutableEntryPoint, ExecutableFunctionArtifact, Runtime,
    jit_abi::{
        JIT_CONSUME_INSTRUCTION_STEP_SYMBOL, JIT_STATUS_OK, JIT_STATUS_RUNTIME_ERROR,
        JIT_VALUE_TAG_BOOL, JIT_VALUE_TAG_I32, JIT_VALUE_TAG_UNIT, JitCompiledFunction, JitValue,
        jit_consume_instruction_step,
    },
    value::Value as RuntimeValue,
};

pub struct CraneliftBackend {
    backend_id: BackendId,
    target: BackendTarget,
    module: JITModule,
    consume_step: FuncId,
    next_symbol: u64,
}

impl fmt::Debug for CraneliftBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CraneliftBackend")
            .field("backend_id", &self.backend_id)
            .field("target", &self.target)
            .field("next_symbol", &self.next_symbol)
            .finish_non_exhaustive()
    }
}

impl CraneliftBackend {
    pub fn for_host() -> Result<Self, CraneliftBackendError> {
        let isa = host_isa()?;
        let target = BackendTarget {
            triple: isa.triple().to_string(),
            pointer_width: isa.pointer_bytes() * 8,
            features: Vec::new(),
        };
        let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
        builder.symbol(
            JIT_CONSUME_INSTRUCTION_STEP_SYMBOL,
            jit_consume_instruction_step as *const u8,
        );
        let mut module = JITModule::new(builder);
        let mut consume_step_sig = module.make_signature();
        consume_step_sig
            .params
            .push(AbiParam::new(module.target_config().pointer_type()));
        consume_step_sig.returns.push(AbiParam::new(types::I32));
        let consume_step = module
            .declare_function(
                JIT_CONSUME_INSTRUCTION_STEP_SYMBOL,
                Linkage::Import,
                &consume_step_sig,
            )
            .map_err(CraneliftBackendError::from_module_error)?;

        Ok(Self {
            backend_id: BackendId::new("cranelift"),
            target,
            module,
            consume_step,
            next_symbol: 0,
        })
    }

    pub fn invoke_compiled_scalar(
        &self,
        artifact: &ExecutableFunctionArtifact,
        runtime: &Runtime,
    ) -> Result<RuntimeValue, CraneliftInvocationError> {
        let ExecutableEntryPoint::Native { address, .. } = artifact.entry else {
            return Err(CraneliftInvocationError::new(
                "artifact does not contain a native entry point",
            ));
        };
        let function: JitCompiledFunction =
            unsafe { mem::transmute::<usize, JitCompiledFunction>(address) };
        let mut result = JitValue::default();
        let status = unsafe { function(runtime as *const Runtime, &mut result) };
        if status != JIT_STATUS_OK {
            return Err(CraneliftInvocationError::new(
                "compiled function reported a runtime helper failure",
            ));
        }
        result.into_value().ok_or_else(|| {
            CraneliftInvocationError::new("compiled function returned bad value tag")
        })
    }

    fn compile_eligible_function(
        &mut self,
        input: &BackendFunctionInput<'_>,
    ) -> Result<ExecutableFunctionArtifact, BackendCompileError> {
        if let Err(error) = verify_module(input.module) {
            return Err(BackendCompileError {
                diagnostics: vec![BackendDiagnostic {
                    kind: BackendDiagnosticKind::InvalidInput,
                    message: format!("bytecode verification failed before JIT lowering: {error:?}"),
                }],
            });
        }
        if input.function.parameter_count != 0 {
            return Err(BackendCompileError::unsupported(format!(
                "Cranelift baseline currently supports only zero-argument functions, `{}` has {}",
                input.function.name, input.function.parameter_count
            )));
        }
        let symbol = self.next_function_symbol(input.module_name, input.function);
        let address = self.emit_scalar_function(&symbol, input.function)?;
        let mut artifact = ExecutableFunctionArtifact::new(
            self.backend_id.clone(),
            self.target.clone(),
            input.function_ref(),
        );
        artifact.entry = ExecutableEntryPoint::Native { symbol, address };
        Ok(artifact)
    }

    fn next_function_symbol(&mut self, module_name: &str, function: &BytecodeFunction) -> String {
        let symbol = format!(
            "kagari_jit_{}_{}_{}",
            sanitize_symbol(module_name),
            function.id.index(),
            self.next_symbol
        );
        self.next_symbol += 1;
        symbol
    }

    fn emit_scalar_function(
        &mut self,
        symbol: &str,
        function: &BytecodeFunction,
    ) -> Result<usize, BackendCompileError> {
        let mut signature = self.module.make_signature();
        let pointer_type = self.module.target_config().pointer_type();
        signature.params.push(AbiParam::new(pointer_type));
        signature.params.push(AbiParam::new(pointer_type));
        signature.returns.push(AbiParam::new(types::I32));
        let function_id = self
            .module
            .declare_function(symbol, Linkage::Local, &signature)
            .map_err(|error| backend_internal_error(format!("declare JIT function: {error}")))?;

        let mut context = Context::new();
        context.func.signature = signature;
        let mut function_context = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut context.func, &mut function_context);
            let entry_block = builder.create_block();
            let helper_error_block = builder.create_block();
            builder.switch_to_block(entry_block);
            builder.append_block_params_for_function_params(entry_block);
            let runtime_ptr = builder.block_params(entry_block)[0];
            let result_ptr = builder.block_params(entry_block)[1];
            let consume_step = self
                .module
                .declare_func_in_func(self.consume_step, &mut builder.func);
            let mut registers = vec![None; usize::from(function.register_count)];
            let mut returned = false;

            for (offset, instruction) in function.instructions.iter().enumerate() {
                emit_resource_check(&mut builder, consume_step, runtime_ptr, helper_error_block);
                match instruction {
                    BytecodeInstruction::LoadConst { dst, constant } => {
                        let value = emit_constant(&mut builder, constant)?;
                        write_register(&mut registers, *dst, value)?;
                    }
                    BytecodeInstruction::Move { dst, src } => {
                        let value = read_register(&registers, *src)?;
                        write_register(&mut registers, *dst, value)?;
                    }
                    BytecodeInstruction::Unary { dst, op, operand } => {
                        let operand = read_register(&registers, *operand)?;
                        let value = emit_unary(&mut builder, *op, operand)?;
                        write_register(&mut registers, *dst, value)?;
                    }
                    BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
                        let lhs = read_register(&registers, *lhs)?;
                        let rhs = read_register(&registers, *rhs)?;
                        let value = emit_binary(&mut builder, *op, lhs, rhs)?;
                        write_register(&mut registers, *dst, value)?;
                    }
                    BytecodeInstruction::Return(value) => {
                        let value = match value {
                            Some(register) => read_register(&registers, *register)?,
                            None => emit_unit(&mut builder),
                        };
                        emit_store_result(&mut builder, result_ptr, value);
                        let ok_status = builder.ins().iconst(types::I32, i64::from(JIT_STATUS_OK));
                        builder.ins().return_(&[ok_status]);
                        if offset + 1 != function.instructions.len() {
                            return Err(BackendCompileError::unsupported(format!(
                                "Cranelift baseline does not support instructions after return in `{}`",
                                function.name
                            )));
                        }
                        returned = true;
                        break;
                    }
                    unsupported => {
                        return Err(BackendCompileError::unsupported(format!(
                            "Cranelift baseline does not support instruction `{unsupported:?}` in `{}`",
                            function.name
                        )));
                    }
                }
            }

            if !returned {
                return Err(BackendCompileError::unsupported(format!(
                    "Cranelift baseline requires an explicit return in `{}`",
                    function.name
                )));
            }

            builder.switch_to_block(helper_error_block);
            let error_status = builder
                .ins()
                .iconst(types::I32, i64::from(JIT_STATUS_RUNTIME_ERROR));
            builder.ins().return_(&[error_status]);
            builder.seal_all_blocks();
            builder.finalize();
        }

        self.module
            .define_function(function_id, &mut context)
            .map_err(|error| backend_internal_error(format!("define JIT function: {error}")))?;
        self.module.clear_context(&mut context);
        self.module
            .finalize_definitions()
            .map_err(|error| backend_internal_error(format!("finalize JIT function: {error}")))?;
        Ok(self.module.get_finalized_function(function_id) as usize)
    }
}

impl CodegenBackend for CraneliftBackend {
    fn backend_id(&self) -> BackendId {
        self.backend_id.clone()
    }

    fn target(&self) -> BackendTarget {
        self.target.clone()
    }

    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<ExecutableFunctionArtifact, BackendCompileError> {
        self.compile_eligible_function(&input)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraneliftBackendError {
    message: String,
}

impl CraneliftBackendError {
    pub fn message(&self) -> &str {
        &self.message
    }

    fn from_module_error(error: cranelift_module::ModuleError) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl fmt::Display for CraneliftBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CraneliftBackendError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraneliftInvocationError {
    message: String,
}

impl CraneliftInvocationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CraneliftInvocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CraneliftInvocationError {}

#[derive(Debug, Clone, Copy)]
struct LoweredValue {
    tag: u8,
    payload: ir::Value,
}

fn host_isa() -> Result<Arc<dyn TargetIsa>, CraneliftBackendError> {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("use_colocated_libcalls", "false")
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })?;
    flag_builder
        .set("is_pic", "false")
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })?;
    let flags = settings::Flags::new(flag_builder);
    cranelift_native::builder()
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })?
        .finish(flags)
        .map_err(|error| CraneliftBackendError {
            message: error.to_string(),
        })
}

fn backend_internal_error(message: impl Into<String>) -> BackendCompileError {
    BackendCompileError {
        diagnostics: vec![BackendDiagnostic {
            kind: BackendDiagnosticKind::InternalError,
            message: message.into(),
        }],
    }
}

fn emit_resource_check(
    builder: &mut FunctionBuilder<'_>,
    consume_step: ir::FuncRef,
    runtime_ptr: ir::Value,
    helper_error_block: ir::Block,
) {
    let call = builder.ins().call(consume_step, &[runtime_ptr]);
    let status = builder.inst_results(call)[0];
    let ok = builder
        .ins()
        .icmp_imm(IntCC::Equal, status, i64::from(JIT_STATUS_OK));
    let continue_block = builder.create_block();
    builder
        .ins()
        .brif(ok, continue_block, &[], helper_error_block, &[]);
    builder.switch_to_block(continue_block);
}

fn emit_constant(
    builder: &mut FunctionBuilder<'_>,
    constant: &ConstantOperand,
) -> Result<LoweredValue, BackendCompileError> {
    match constant {
        ConstantOperand::Unit => Ok(emit_unit(builder)),
        ConstantOperand::Bool(value) => {
            let payload = builder.ins().iconst(types::I64, i64::from(*value as u8));
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_BOOL,
                payload,
            })
        }
        ConstantOperand::I32(value) => {
            let payload = builder.ins().iconst(types::I64, i64::from(*value));
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_I32,
                payload,
            })
        }
        unsupported => Err(BackendCompileError::unsupported(format!(
            "Cranelift baseline does not support constant `{unsupported:?}`"
        ))),
    }
}

fn emit_unit(builder: &mut FunctionBuilder<'_>) -> LoweredValue {
    LoweredValue {
        tag: JIT_VALUE_TAG_UNIT,
        payload: builder.ins().iconst(types::I64, 0),
    }
}

fn emit_unary(
    builder: &mut FunctionBuilder<'_>,
    op: UnaryOp,
    operand: LoweredValue,
) -> Result<LoweredValue, BackendCompileError> {
    match (op, operand.tag) {
        (UnaryOp::Neg, JIT_VALUE_TAG_I32) => {
            let zero = builder.ins().iconst(types::I64, 0);
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_I32,
                payload: builder.ins().isub(zero, operand.payload),
            })
        }
        (UnaryOp::Not, JIT_VALUE_TAG_BOOL) => {
            let is_false = builder.ins().icmp_imm(IntCC::Equal, operand.payload, 0);
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_BOOL,
                payload: builder.ins().uextend(types::I64, is_false),
            })
        }
        _ => Err(BackendCompileError::unsupported(format!(
            "Cranelift baseline does not support unary `{op:?}` for tag {}",
            operand.tag
        ))),
    }
}

fn emit_binary(
    builder: &mut FunctionBuilder<'_>,
    op: BinaryOp,
    lhs: LoweredValue,
    rhs: LoweredValue,
) -> Result<LoweredValue, BackendCompileError> {
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul
            if lhs.tag == JIT_VALUE_TAG_I32 && rhs.tag == JIT_VALUE_TAG_I32 =>
        {
            let payload = match op {
                BinaryOp::Add => builder.ins().iadd(lhs.payload, rhs.payload),
                BinaryOp::Sub => builder.ins().isub(lhs.payload, rhs.payload),
                BinaryOp::Mul => builder.ins().imul(lhs.payload, rhs.payload),
                _ => unreachable!(),
            };
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_I32,
                payload,
            })
        }
        BinaryOp::Eq | BinaryOp::NotEq
            if lhs.tag == rhs.tag && is_comparable_scalar_tag(lhs.tag) =>
        {
            let condition = if op == BinaryOp::Eq {
                IntCC::Equal
            } else {
                IntCC::NotEqual
            };
            let comparison = builder.ins().icmp(condition, lhs.payload, rhs.payload);
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_BOOL,
                payload: builder.ins().uextend(types::I64, comparison),
            })
        }
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge
            if lhs.tag == JIT_VALUE_TAG_I32 && rhs.tag == JIT_VALUE_TAG_I32 =>
        {
            let condition = match op {
                BinaryOp::Lt => IntCC::SignedLessThan,
                BinaryOp::Gt => IntCC::SignedGreaterThan,
                BinaryOp::Le => IntCC::SignedLessThanOrEqual,
                BinaryOp::Ge => IntCC::SignedGreaterThanOrEqual,
                _ => unreachable!(),
            };
            let comparison = builder.ins().icmp(condition, lhs.payload, rhs.payload);
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_BOOL,
                payload: builder.ins().uextend(types::I64, comparison),
            })
        }
        _ => Err(BackendCompileError::unsupported(format!(
            "Cranelift baseline does not support binary `{op:?}` for tags {} and {}",
            lhs.tag, rhs.tag
        ))),
    }
}

fn is_comparable_scalar_tag(tag: u8) -> bool {
    matches!(
        tag,
        JIT_VALUE_TAG_UNIT | JIT_VALUE_TAG_BOOL | JIT_VALUE_TAG_I32
    )
}

fn emit_store_result(
    builder: &mut FunctionBuilder<'_>,
    result_ptr: ir::Value,
    value: LoweredValue,
) {
    let flags = MemFlags::new();
    let tag = builder.ins().iconst(types::I8, i64::from(value.tag));
    builder.ins().store(flags, tag, result_ptr, 0);
    builder.ins().store(flags, value.payload, result_ptr, 8);
}

fn read_register(
    registers: &[Option<LoweredValue>],
    register: Register,
) -> Result<LoweredValue, BackendCompileError> {
    registers
        .get(register.index())
        .and_then(|value| *value)
        .ok_or_else(|| {
            BackendCompileError::unsupported(format!(
                "Cranelift baseline cannot read uninitialized register {}",
                register.index()
            ))
        })
}

fn write_register(
    registers: &mut [Option<LoweredValue>],
    register: Register,
    value: LoweredValue,
) -> Result<(), BackendCompileError> {
    let Some(slot) = registers.get_mut(register.index()) else {
        return Err(BackendCompileError::unsupported(format!(
            "Cranelift baseline cannot write register {} beyond frame layout",
            register.index()
        )));
    };
    *slot = Some(value);
    Ok(())
}

fn sanitize_symbol(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_ir::{
        bytecode::{
            BytecodeFunction, BytecodeInstruction, BytecodeModule, ConstantOperand,
            FunctionMetadata, FunctionRecord, FunctionRef, Register,
        },
        module::ValueType,
    };
    use kagari_runtime::{
        BackendDiagnosticKind, BackendFunctionInput, LoadedModule, ModuleId,
        ReloadDependencySnapshot, ResourcePolicy, RuntimeConfig,
    };

    #[test]
    fn cranelift_backend_initializes_host_target_without_leaking_backend_types() {
        let backend =
            CraneliftBackend::for_host().expect("host Cranelift target should initialize");
        let target = backend.target();

        assert_eq!(backend.backend_id().as_str(), "cranelift");
        assert!(!target.triple.is_empty());
        assert!(matches!(target.pointer_width, 32 | 64));
    }

    #[test]
    fn cranelift_backend_compiles_and_invokes_scalar_bytecode() {
        let mut backend =
            CraneliftBackend::for_host().expect("host Cranelift target should initialize");
        let loaded = loaded_module(function(
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
                BytecodeInstruction::Return(Some(Register::new(2))),
            ],
            ValueType::I32,
            vec![ValueType::I32, ValueType::I32, ValueType::I32],
        ));
        let dependencies = ReloadDependencySnapshot::from_bytecode(&loaded.bytecode);

        let artifact = backend
            .compile_function(input_for(&loaded, dependencies))
            .expect("eligible scalar bytecode should compile");

        assert!(matches!(
            artifact.entry,
            ExecutableEntryPoint::Native { address, .. } if address != 0
        ));
        let runtime = Runtime::new(RuntimeConfig::default());
        let value = backend
            .invoke_compiled_scalar(&artifact, &runtime)
            .expect("compiled scalar function should execute");
        assert_eq!(value, RuntimeValue::I32(42));
        assert_eq!(runtime.resources().counters().instruction_steps, 4);
    }

    #[test]
    fn cranelift_backend_calls_runtime_resource_helper_and_reports_failure() {
        let mut backend =
            CraneliftBackend::for_host().expect("host Cranelift target should initialize");
        let loaded = loaded_module(function(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::Bool(true),
                },
                BytecodeInstruction::Return(Some(Register::new(0))),
            ],
            ValueType::Bool,
            vec![ValueType::Bool],
        ));
        let dependencies = ReloadDependencySnapshot::from_bytecode(&loaded.bytecode);
        let artifact = backend
            .compile_function(input_for(&loaded, dependencies))
            .expect("eligible scalar bytecode should compile");
        let runtime = Runtime::new(RuntimeConfig {
            resources: ResourcePolicy {
                max_instruction_steps: Some(1),
                ..ResourcePolicy::default()
            },
            ..RuntimeConfig::default()
        });

        let error = backend
            .invoke_compiled_scalar(&artifact, &runtime)
            .expect_err("resource helper failure should be visible to the ABI caller");

        assert!(error.message().contains("runtime helper failure"));
        assert_eq!(runtime.resources().counters().instruction_steps, 1);
    }

    #[test]
    fn cranelift_backend_reports_unsupported_instructions_for_fallback_step() {
        let mut backend =
            CraneliftBackend::for_host().expect("host Cranelift target should initialize");
        let loaded = loaded_module(function(
            "main",
            vec![BytecodeInstruction::Return(None)],
            ValueType::Unit,
            Vec::new(),
        ));
        let mut unsupported = loaded.clone();
        unsupported.bytecode.functions[0].instructions.insert(
            0,
            BytecodeInstruction::MakeArray {
                dst: Register::new(0),
                elements: Vec::new(),
            },
        );
        unsupported.bytecode.functions[0].register_count = 1;
        unsupported.bytecode.functions[0].metadata.registers = vec![ValueType::HeapObject];
        let dependencies = ReloadDependencySnapshot::from_bytecode(&unsupported.bytecode);

        let error = backend
            .compile_function(input_for(&unsupported, dependencies))
            .expect_err("unsupported instructions should remain a backend diagnostic");

        assert_eq!(
            error.diagnostics[0].kind,
            BackendDiagnosticKind::UnsupportedFunction
        );
    }

    fn input_for(
        loaded: &LoadedModule,
        dependencies: ReloadDependencySnapshot,
    ) -> BackendFunctionInput<'_> {
        BackendFunctionInput {
            module_key: loaded.key(),
            module_name: &loaded.name,
            module: &loaded.bytecode,
            function: &loaded.bytecode.functions[0],
            dependencies,
        }
    }

    fn loaded_module(function: BytecodeFunction) -> LoadedModule {
        let mut module = BytecodeModule::default();
        module.types = vec![
            ValueType::Unit,
            ValueType::Bool,
            ValueType::I32,
            ValueType::HeapObject,
        ];
        module.constants = constants_for_function(&function);
        module.function_table.push(FunctionRecord {
            id: function.id,
            name: function.name.clone(),
            params: function.metadata.params.clone(),
            return_type: function.metadata.return_type,
            effects: function.metadata.effects,
        });
        module.functions.push(function);
        LoadedModule {
            id: ModuleId::new(0),
            name: "jit_test".to_owned(),
            epoch: kagari_runtime::reload::ModuleEpoch(0),
            bytecode: module,
        }
    }

    fn function(
        name: &str,
        instructions: Vec<BytecodeInstruction>,
        return_type: ValueType,
        registers: Vec<ValueType>,
    ) -> BytecodeFunction {
        let register_count = registers.len() as u16;
        BytecodeFunction {
            id: FunctionRef::new(0),
            name: name.to_owned(),
            register_count,
            metadata: FunctionMetadata {
                return_type,
                registers,
                ..FunctionMetadata::default()
            },
            instructions,
            ..BytecodeFunction::default()
        }
    }

    fn constants_for_function(function: &BytecodeFunction) -> Vec<ConstantOperand> {
        let mut constants = Vec::new();
        for instruction in &function.instructions {
            let BytecodeInstruction::LoadConst { constant, .. } = instruction else {
                continue;
            };
            if !constants.contains(constant) {
                constants.push(constant.clone());
            }
        }
        constants
    }
}
