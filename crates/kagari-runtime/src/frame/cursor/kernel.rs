//! Closed scalar operations reuse authority without admitting callbacks or values.
use crate::{
    error::RuntimeError,
    frame::cursor::ExecutionCursor,
    module::execution::{ExecutionInstruction, OperandSlot, ScalarConstant},
    numeric,
    value::Value,
};
use kagari_bytecode::instruction::BinaryOp;

pub enum RegionExit {
    Safepoint,
    Boundary,
    Return(Value),
}

enum CursorProgress {
    Continue,
    Boundary,
    Return(Value),
}

impl ExecutionCursor<'_> {
    /// Execute sealed scalar operations under one authority check. The driver
    /// has already polled and observed the first PC; subsequent logical PCs keep
    /// the same cancellation, collection and observation boundaries. No caller
    /// supplies values or callbacks while authority is reused.
    pub fn execute_region(&mut self) -> Result<RegionExit, RuntimeError> {
        self.runtime
            .resources()
            .ensure_cursor_allowed(&self.session)?;
        self.runtime.gc().ensure_no_native_borrow()?;
        let mut first = true;
        loop {
            if !first && self.prepare_instruction()? {
                return Ok(RegionExit::Safepoint);
            }
            first = false;
            match self.execute_next()? {
                CursorProgress::Continue => {}
                CursorProgress::Boundary => return Ok(RegionExit::Boundary),
                CursorProgress::Return(value) => return Ok(RegionExit::Return(value)),
            }
        }
    }

    fn execute_next(&mut self) -> Result<CursorProgress, RuntimeError> {
        let instruction = self.next_instruction().ok_or_else(|| {
            self.runtime
                .resources()
                .quarantine("verified function fell through")
        })?;
        let value = match instruction {
            ExecutionInstruction::Constant { dst, value } => {
                if !self.scalar_destination(dst)? {
                    return Ok(CursorProgress::Boundary);
                }
                (dst, scalar_value(value))
            }
            ExecutionInstruction::Move { dst, src } => {
                let Some(value) = self.scalar_operand(src)? else {
                    return Ok(CursorProgress::Boundary);
                };
                if !self.scalar_destination(dst)? {
                    return Ok(CursorProgress::Boundary);
                }
                (dst, value)
            }
            ExecutionInstruction::Unary { dst, op, operand } => {
                let Some(value) = self.scalar_operand(operand)? else {
                    return Ok(CursorProgress::Boundary);
                };
                // Check the destination before an operation can trap: dispatching
                // a cold cleanup must not execute the arithmetic twice.
                if !self.scalar_destination(dst)? {
                    return Ok(CursorProgress::Boundary);
                }
                (dst, numeric::unary(op, value)?)
            }
            ExecutionInstruction::Binary { dst, op, lhs, rhs } => {
                if matches!(op, BinaryOp::IdentityEq | BinaryOp::IdentityNotEq) {
                    return Ok(CursorProgress::Boundary);
                }
                let (Some(lhs), Some(rhs)) = (self.scalar_operand(lhs)?, self.scalar_operand(rhs)?)
                else {
                    return Ok(CursorProgress::Boundary);
                };
                if !self.scalar_destination(dst)? {
                    return Ok(CursorProgress::Boundary);
                }
                (dst, numeric::scalar_binary(op, lhs, rhs)?)
            }
            ExecutionInstruction::Convert {
                dst,
                src,
                conversion,
            } => {
                let Some(value) = self.scalar_operand(src)? else {
                    return Ok(CursorProgress::Boundary);
                };
                if !self.scalar_destination(dst)? {
                    return Ok(CursorProgress::Boundary);
                }
                (dst, numeric::convert(conversion, value)?)
            }
            ExecutionInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => {
                let Some(lhs) = self.scalar_operand(lhs)? else {
                    return Ok(CursorProgress::Boundary);
                };
                let rhs = match rhs {
                    Some(slot) => match self.scalar_operand(slot)? {
                        Some(value) => Some(value),
                        None => return Ok(CursorProgress::Boundary),
                    },
                    None => None,
                };
                if !self.scalar_destination(dst)? {
                    return Ok(CursorProgress::Boundary);
                }
                (dst, numeric::fixed_integer(operation, lhs, rhs)?)
            }
            ExecutionInstruction::Jump(target) => {
                self.jump(target.index())?;
                return Ok(CursorProgress::Continue);
            }
            ExecutionInstruction::Branch {
                cond,
                then_target,
                else_target,
            } => {
                let target = match self.scalar_operand(cond)? {
                    Some(Value::Bool(true)) => then_target,
                    Some(Value::Bool(false)) => else_target,
                    _ => {
                        return Err(self
                            .runtime
                            .resources()
                            .quarantine("verified branch condition is not bool"));
                    }
                };
                self.jump(target.index())?;
                return Ok(CursorProgress::Continue);
            }
            ExecutionInstruction::Return(register) => {
                let value = match register {
                    Some(slot) => match self.scalar_operand(slot)? {
                        Some(value) => value,
                        None => return Ok(CursorProgress::Boundary),
                    },
                    None => Value::Unit,
                };
                return Ok(CursorProgress::Return(value));
            }
            ExecutionInstruction::Boundary => return Ok(CursorProgress::Boundary),
        };
        let (dst, value) = value;
        // Every branch above constructs a plain scalar. The previous value is
        // also plain, so replacement cannot invoke external code or change roots.
        self.values[dst.index()] = value;
        Ok(CursorProgress::Continue)
    }

    fn scalar_operand(&self, slot: OperandSlot) -> Result<Option<Value>, RuntimeError> {
        let value = self
            .values
            .get(slot.index())
            .ok_or_else(|| self.invalid())?;
        Ok(plain_scalar(value).map(scalar_value))
    }

    fn scalar_destination(&self, slot: OperandSlot) -> Result<bool, RuntimeError> {
        self.values
            .get(slot.index())
            .map(|value| plain_scalar(value).is_some())
            .ok_or_else(|| self.invalid())
    }
}

fn plain_scalar(value: &Value) -> Option<ScalarConstant> {
    Some(match *value {
        Value::Unit => ScalarConstant::Unit,
        Value::Bool(v) => ScalarConstant::Bool(v),
        Value::I32(v) => ScalarConstant::I32(v),
        Value::I64(v) => ScalarConstant::I64(v),
        Value::U64(v) => ScalarConstant::U64(v),
        Value::F32(v) => ScalarConstant::F32(v),
        Value::F64(v) => ScalarConstant::F64(v),
        _ => return None,
    })
}

fn scalar_value(value: ScalarConstant) -> Value {
    match value {
        ScalarConstant::Unit => Value::Unit,
        ScalarConstant::Bool(v) => Value::Bool(v),
        ScalarConstant::I32(v) => Value::I32(v),
        ScalarConstant::I64(v) => Value::I64(v),
        ScalarConstant::U64(v) => Value::U64(v),
        ScalarConstant::F32(v) => Value::F32(v),
        ScalarConstant::F64(v) => Value::F64(v),
    }
}
