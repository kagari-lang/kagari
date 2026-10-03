//! MIR-to-CLIF emission and per-product executable memory ownership.
use crate::{
    internal_error,
    scalar::{
        emit_binary, emit_constant, emit_resource_check, emit_store_result, emit_unary, emit_unit,
        read_register, write_register,
    },
};
use kagari_common::identity::table::DefinitionId;

use cranelift_codegen::{
    Context,
    ir::{AbiParam, InstBuilder, types},
    isa::TargetIsa,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module, default_libcall_names};
use kagari_abi::{
    ids::FunctionRef,
    native::{
        ExecutableEntryPoint, ExecutableFunctionArtifact, ExecutableSafepoint,
        ExecutableSafepointKind, ExecutableStackMap, ExecutableTrap, NativeCodeOwner,
        NativeCompilationProduct, NativeType,
    },
    native_call::{JIT_POLL_EXECUTION_SYMBOL, JIT_STATUS_INTEGER_OVERFLOW, JIT_STATUS_OK},
    operations::{BinaryOp, UnaryOp},
    representation::ValueType,
};
use kagari_codegen::{BackendConfiguration, BackendFunctionInput, diagnostic::BackendCompileError};
use kagari_mir::{
    function::MirFunction,
    instruction::{Instruction, Terminator},
};
use std::{fmt, rc::Rc, sync::Arc};

struct CodeMemory(Option<JITModule>);

impl fmt::Debug for CodeMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CraneliftCodeMemory")
            .finish_non_exhaustive()
    }
}

impl NativeCodeOwner for CodeMemory {}

impl Drop for CodeMemory {
    fn drop(&mut self) {
        if let Some(module) = self.0.take() {
            // SAFETY: the product's last owner is gone. Installed handles retain
            // that owner throughout invocation, including synchronous reentry.
            unsafe {
                module.free_memory();
            }
        }
    }
}

pub(super) fn compile(
    isa: Arc<dyn TargetIsa>,
    configuration: &BackendConfiguration,
    input: BackendFunctionInput<'_>,
) -> Result<NativeCompilationProduct, BackendCompileError> {
    let function = input.function();
    check_subset(function)?;
    let helpers = &input.links().helpers;
    let mut matching = helpers
        .iter()
        .filter(|helper| helper.symbol == JIT_POLL_EXECUTION_SYMBOL);
    let helper = matching
        .next()
        .ok_or_else(|| internal_error("missing execution polling helper"))?;
    if matching.next().is_some()
        || helper.address == 0
        || helper.parameters != [NativeType::Pointer, NativeType::I64]
        || helper.results != [NativeType::I32]
    {
        return Err(internal_error(
            "invalid execution polling helper declaration",
        ));
    }
    let mut jit = JITBuilder::with_isa(isa, default_libcall_names());
    jit.symbol(&helper.symbol, helper.address as *const u8);
    let mut memory = CodeMemory(Some(JITModule::new(jit)));
    let module = memory.0.as_mut().expect("new code owner");
    let pointer_type = module.target_config().pointer_type();
    let mut helper_sig = module.make_signature();
    helper_sig
        .params
        .extend([AbiParam::new(pointer_type), AbiParam::new(types::I64)]);
    helper_sig.returns.push(AbiParam::new(types::I32));
    let helper_id = module
        .declare_function(&helper.symbol, Linkage::Import, &helper_sig)
        .map_err(|e| internal_error(format!("declare helper: {e}")))?;
    // Each product has an independent module, so symbols need no global counter.
    let symbol = format!("kagari_jit_{}", function.id.index());
    let mut signature = module.make_signature();
    signature
        .params
        .extend([AbiParam::new(pointer_type), AbiParam::new(pointer_type)]);
    signature.returns.push(AbiParam::new(types::I32));
    let id = module
        .declare_function(&symbol, Linkage::Local, &signature)
        .map_err(|e| internal_error(format!("declare native entry: {e}")))?;
    let mut context = Context::new();
    context.func.signature = signature;
    let mut frontend = FunctionBuilderContext::new();
    let facts = input
        .analysis()
        .block(function.entry)
        .expect("sealed entry facts");
    let block = &function.blocks[function.entry.index()];
    let mut safepoints = Vec::new();
    let mut traps = Vec::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut frontend);
        let entry = builder.create_block();
        let helper_error = builder.create_block();
        builder.append_block_param(helper_error, types::I32);
        let overflow = builder.create_block();
        builder.switch_to_block(entry);
        builder.append_block_params_for_function_params(entry);
        let runtime_ptr = builder.block_params(entry)[0];
        let result_ptr = builder.block_params(entry)[1];
        let consume_step = module.declare_func_in_func(helper_id, builder.func);
        let mut temps = vec![None; function.temps.len()];
        for (index, instruction) in block.instructions.iter().enumerate() {
            let point = facts.instruction(index).expect("sealed point facts");
            emit_resource_check(
                &mut builder,
                consume_step,
                runtime_ptr,
                helper_error,
                point.logical_offset(),
            );
            safepoints.push(safepoint(point.logical_offset()));
            match instruction {
                Instruction::LoadConst { dst, constant } => {
                    let value = emit_constant(&mut builder, constant)?;
                    write_register(&mut temps, dst.temp, value)?;
                }
                Instruction::Move { dst, src } => {
                    let value = read_register(&temps, src.temp)?;
                    write_register(&mut temps, dst.temp, value)?;
                }
                Instruction::Unary { dst, op, operand } => {
                    let value = emit_unary(
                        &mut builder,
                        *op,
                        read_register(&temps, operand.temp)?,
                        overflow,
                    )?;
                    write_register(&mut temps, dst.temp, value)?;
                }
                Instruction::Binary { dst, op, lhs, rhs } => {
                    let value = emit_binary(
                        &mut builder,
                        *op,
                        read_register(&temps, lhs.temp)?,
                        read_register(&temps, rhs.temp)?,
                        overflow,
                    )?;
                    write_register(&mut temps, dst.temp, value)?;
                }
                _ => unreachable!("checked native subset"),
            }
            if matches!(
                instruction,
                Instruction::Unary {
                    op: UnaryOp::Neg,
                    ..
                } | Instruction::Binary {
                    op: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul,
                    ..
                }
            ) {
                traps.push(ExecutableTrap {
                    instruction_offset: point.logical_offset(),
                    reason: "integer overflow".into(),
                });
            }
        }
        let point = facts.terminator();
        emit_resource_check(
            &mut builder,
            consume_step,
            runtime_ptr,
            helper_error,
            point.logical_offset(),
        );
        safepoints.push(safepoint(point.logical_offset()));
        let Some(Terminator::Return(value)) = &block.terminator else {
            unreachable!("checked scalar return")
        };
        let value = match value {
            Some(value) => read_register(&temps, value.temp)?,
            None => emit_unit(&mut builder),
        };
        emit_store_result(&mut builder, result_ptr, value);
        let ok = builder.ins().iconst(types::I32, i64::from(JIT_STATUS_OK));
        builder.ins().return_(&[ok]);
        builder.switch_to_block(helper_error);
        let status = builder.block_params(helper_error)[0];
        builder.ins().return_(&[status]);
        builder.switch_to_block(overflow);
        let status = builder
            .ins()
            .iconst(types::I32, i64::from(JIT_STATUS_INTEGER_OVERFLOW));
        builder.ins().return_(&[status]);
        builder.seal_all_blocks();
        builder.finalize();
    }
    module
        .define_function(id, &mut context)
        .map_err(|e| internal_error(format!("define native entry: {e}")))?;
    module.clear_context(&mut context);
    module
        .finalize_definitions()
        .map_err(|e| internal_error(format!("finalize native entry: {e}")))?;
    let address = module.get_finalized_function(id) as usize;
    let mut artifact = ExecutableFunctionArtifact::new(
        configuration.backend.clone(),
        configuration.target.clone(),
        FunctionRef::new(function.id.index()),
    );
    artifact.entry = ExecutableEntryPoint::Native { symbol, address };
    artifact.safepoints = safepoints;
    artifact.traps = traps;
    Ok(NativeCompilationProduct {
        artifact,
        owner: Rc::new(memory),
    })
}

fn safepoint(instruction_offset: usize) -> ExecutableSafepoint {
    ExecutableSafepoint {
        instruction_offset,
        kind: ExecutableSafepointKind::RuntimeHelperCall {
            helper: JIT_POLL_EXECUTION_SYMBOL.into(),
        },
        stack_map: ExecutableStackMap::empty(),
    }
}

fn check_subset(function: &MirFunction<DefinitionId>) -> Result<(), BackendCompileError> {
    let scalar = |ty| matches!(ty, ValueType::Unit | ValueType::Bool | ValueType::I32);
    if !function.params.is_empty() {
        return Err(BackendCompileError::unsupported(
            "Cranelift baseline requires zero arguments",
        ));
    }
    if !scalar(function.return_type)
        || function.locals.iter().any(|l| !scalar(l.ty))
        || function.temps.iter().any(|t| !scalar(t.ty))
    {
        return Err(BackendCompileError::unsupported(
            "Cranelift baseline cannot emit precise stack maps for non-scalar slots",
        ));
    }
    if function.blocks.len() != 1
        || !matches!(
            function.blocks[function.entry.index()].terminator,
            Some(Terminator::Return(_))
        )
    {
        return Err(BackendCompileError::unsupported(
            "Cranelift baseline does not support control flow",
        ));
    }
    for instruction in &function.blocks[function.entry.index()].instructions {
        if !matches!(
            instruction,
            Instruction::LoadConst { .. }
                | Instruction::Move { .. }
                | Instruction::Unary { .. }
                | Instruction::Binary { .. }
        ) {
            return Err(BackendCompileError::unsupported(format!(
                "Cranelift baseline does not support instruction `{instruction:?}`"
            )));
        }
    }
    Ok(())
}
