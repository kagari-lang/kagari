//! Scalar legalization for the existing Unit/Bool/i32 native subset.
use cranelift_codegen::ir::{self, InstBuilder, MemFlags, condcodes::IntCC, types};
use cranelift_frontend::FunctionBuilder;
use kagari_abi::{
    native_call::{
        JIT_STATUS_OK, JIT_VALUE_TAG_BOOL, JIT_VALUE_TAG_I32, JIT_VALUE_TAG_UNIT, JitValue,
    },
    operations::{BinaryOp, UnaryOp},
};
use kagari_codegen::diagnostic::BackendCompileError;
use kagari_mir::{ids::TempId, instruction::Constant};
use std::mem::offset_of;

#[derive(Debug, Clone, Copy)]
pub(super) struct LoweredValue {
    tag: u8,
    payload: ir::Value,
}

pub(super) fn emit_resource_check(
    builder: &mut FunctionBuilder<'_>,
    consume_step: ir::FuncRef,
    runtime_ptr: ir::Value,
    helper_error_block: ir::Block,
    offset: usize,
) {
    let offset = builder.ins().iconst(types::I64, offset as i64);
    let call = builder.ins().call(consume_step, &[runtime_ptr, offset]);
    let status = builder.inst_results(call)[0];
    let ok = builder
        .ins()
        .icmp_imm(IntCC::Equal, status, i64::from(JIT_STATUS_OK));
    let continue_block = builder.create_block();
    builder.ins().brif(
        ok,
        continue_block,
        &[],
        helper_error_block,
        &[status.into()],
    );
    builder.switch_to_block(continue_block);
}

pub(super) fn emit_constant(
    builder: &mut FunctionBuilder<'_>,
    constant: &Constant,
) -> Result<LoweredValue, BackendCompileError> {
    match constant {
        Constant::Unit => Ok(emit_unit(builder)),
        Constant::Bool(value) => {
            let payload = builder.ins().iconst(types::I64, i64::from(*value as u8));
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_BOOL,
                payload,
            })
        }
        Constant::I32(value) => {
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

pub(super) fn emit_unit(builder: &mut FunctionBuilder<'_>) -> LoweredValue {
    LoweredValue {
        tag: JIT_VALUE_TAG_UNIT,
        payload: builder.ins().iconst(types::I64, 0),
    }
}

pub(super) fn emit_unary(
    builder: &mut FunctionBuilder<'_>,
    op: UnaryOp,
    operand: LoweredValue,
    overflow_block: ir::Block,
) -> Result<LoweredValue, BackendCompileError> {
    match (op, operand.tag) {
        (UnaryOp::Neg, JIT_VALUE_TAG_I32) => {
            let zero = builder.ins().iconst(types::I64, 0);
            let payload = builder.ins().isub(zero, operand.payload);
            emit_i32_range_check(builder, payload, overflow_block);
            Ok(LoweredValue {
                tag: JIT_VALUE_TAG_I32,
                payload,
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

pub(super) fn emit_binary(
    builder: &mut FunctionBuilder<'_>,
    op: BinaryOp,
    lhs: LoweredValue,
    rhs: LoweredValue,
    overflow_block: ir::Block,
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
            emit_i32_range_check(builder, payload, overflow_block);
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

fn emit_i32_range_check(
    builder: &mut FunctionBuilder<'_>,
    value: ir::Value,
    overflow_block: ir::Block,
) {
    // Inputs are sign-extended i32. Their sum, difference, product and negation
    // fit i64, so checking the widened result detects each i32 overflow before
    // any later instruction can observe it or hide it through another operation.
    let below = builder
        .ins()
        .icmp_imm(IntCC::SignedLessThan, value, i64::from(i32::MIN));
    let above = builder
        .ins()
        .icmp_imm(IntCC::SignedGreaterThan, value, i64::from(i32::MAX));
    let overflow = builder.ins().bor(below, above);
    let next = builder.create_block();
    builder.ins().brif(overflow, overflow_block, &[], next, &[]);
    builder.switch_to_block(next);
}

fn is_comparable_scalar_tag(tag: u8) -> bool {
    matches!(
        tag,
        JIT_VALUE_TAG_UNIT | JIT_VALUE_TAG_BOOL | JIT_VALUE_TAG_I32
    )
}

pub(super) fn emit_store_result(
    builder: &mut FunctionBuilder<'_>,
    result_ptr: ir::Value,
    value: LoweredValue,
) {
    let flags = MemFlags::new();
    let tag = builder.ins().iconst(types::I8, i64::from(value.tag));
    builder
        .ins()
        .store(flags, tag, result_ptr, offset_of!(JitValue, tag) as i32);
    builder.ins().store(
        flags,
        value.payload,
        result_ptr,
        offset_of!(JitValue, payload) as i32,
    );
}

pub(super) fn read_register(
    registers: &[Option<LoweredValue>],
    register: TempId,
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

pub(super) fn write_register(
    registers: &mut [Option<LoweredValue>],
    register: TempId,
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
