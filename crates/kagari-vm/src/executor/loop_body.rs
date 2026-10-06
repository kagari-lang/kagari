//! Run non-reentrant operations with one frame/window borrow. Slow boundaries
//! publish the logical PC and release all borrows before entering the runtime.
use crate::{error::VmError, executor::Executor};
use kagari_runtime::{
    module::execution::{ExecutionInstruction, ScalarConstant},
    numeric,
    value::Value,
};

pub(super) enum LoopExit {
    Safepoint,
    Boundary,
    Return(Value),
}

impl Executor<'_> {
    pub(super) fn run_cursor(&self) -> Result<LoopExit, VmError> {
        let mut frame = self.stack.cursor(self.runtime)?;
        // The caller has already polled, collected and observed this first PC.
        let mut first = true;
        loop {
            if !first && frame.prepare_instruction()? {
                return Ok(LoopExit::Safepoint);
            }
            first = false;
            let instruction = frame
                .next_instruction()
                .ok_or(VmError::UnsupportedInstruction(
                    "verified function fell through",
                ))?;
            match instruction {
                ExecutionInstruction::Constant { dst, value } => {
                    frame.write_operand(
                        dst,
                        match value {
                            ScalarConstant::Unit => Value::Unit,
                            ScalarConstant::Bool(v) => Value::Bool(v),
                            ScalarConstant::I32(v) => Value::I32(v),
                            ScalarConstant::I64(v) => Value::I64(v),
                            ScalarConstant::U64(v) => Value::U64(v),
                            ScalarConstant::F32(v) => Value::F32(v),
                            ScalarConstant::F64(v) => Value::F64(v),
                        },
                    )?;
                }
                ExecutionInstruction::Move { dst, src } => {
                    let value = frame.read_operand(src)?;
                    frame.write_operand(dst, value)?;
                }
                ExecutionInstruction::Unary { dst, op, operand } => {
                    let value = frame.read_operand(operand)?;
                    frame.write_operand(dst, Self::apply_unary(op, value)?)?;
                }
                ExecutionInstruction::Binary { dst, op, lhs, rhs } => {
                    let lhs = frame.read_operand(lhs)?;
                    let rhs = frame.read_operand(rhs)?;
                    frame.write_operand(dst, self.apply_binary(op, lhs, rhs)?)?;
                }
                ExecutionInstruction::Convert {
                    dst,
                    src,
                    conversion,
                } => {
                    let value = frame.read_operand(src)?;
                    frame.write_operand(dst, numeric::convert(conversion, value)?)?;
                }
                ExecutionInstruction::Numeric {
                    dst,
                    operation,
                    lhs,
                    rhs,
                } => {
                    let lhs = frame.read_operand(lhs)?;
                    let rhs = rhs.map(|r| frame.read_operand(r)).transpose()?;
                    frame.write_operand(dst, numeric::fixed_integer(operation, lhs, rhs)?)?;
                }
                ExecutionInstruction::Jump(target) => frame.jump_to(target.index())?,
                ExecutionInstruction::Branch {
                    cond,
                    then_target,
                    else_target,
                } => {
                    let target = match frame.read_operand(cond)? {
                        Value::Bool(true) => then_target,
                        Value::Bool(false) => else_target,
                        _ => return Err(VmError::InvalidBranchCondition),
                    };
                    frame.jump_to(target.index())?;
                }
                ExecutionInstruction::Return(register) => {
                    let value = register
                        .map(|r| frame.read_operand(r))
                        .transpose()?
                        .unwrap_or(Value::Unit);
                    return Ok(LoopExit::Return(value));
                }
                ExecutionInstruction::Boundary => return Ok(LoopExit::Boundary),
            }
        }
    }
}
