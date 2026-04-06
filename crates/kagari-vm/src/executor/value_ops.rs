use kagari_ir::bytecode::{BinaryOp, ConstantOperand, UnaryOp};
use kagari_runtime::value::Value;

use crate::error::VmError;
use crate::executor::Executor;

impl Executor<'_> {
    pub(crate) fn constant_to_value(constant: ConstantOperand) -> Value {
        match constant {
            ConstantOperand::Unit => Value::Unit,
            ConstantOperand::Bool(value) => Value::Bool(value),
            ConstantOperand::I32(value) => Value::I32(value),
            ConstantOperand::F32(value) => Value::F32(value),
            ConstantOperand::Str(value) => Value::Str(value),
        }
    }

    pub(crate) fn apply_unary(op: UnaryOp, value: Value) -> Result<Value, VmError> {
        match (op, value) {
            (UnaryOp::Neg, Value::I32(value)) => Ok(Value::I32(-value)),
            (UnaryOp::Neg, Value::F32(value)) => Ok(Value::F32(-value)),
            (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
            (UnaryOp::Neg, _) => Err(VmError::TypeMismatch("unary neg expects numeric value")),
            (UnaryOp::Not, _) => Err(VmError::TypeMismatch("unary not expects bool value")),
        }
    }

    pub(crate) fn apply_binary(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, VmError> {
        match op {
            BinaryOp::Add => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::I32(lhs + rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::F32(lhs + rhs)),
                _ => Err(VmError::TypeMismatch(
                    "add expects matching numeric operands",
                )),
            },
            BinaryOp::Sub => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::I32(lhs - rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::F32(lhs - rhs)),
                _ => Err(VmError::TypeMismatch(
                    "sub expects matching numeric operands",
                )),
            },
            BinaryOp::Mul => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::I32(lhs * rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::F32(lhs * rhs)),
                _ => Err(VmError::TypeMismatch(
                    "mul expects matching numeric operands",
                )),
            },
            BinaryOp::Div => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::I32(lhs / rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::F32(lhs / rhs)),
                _ => Err(VmError::TypeMismatch(
                    "div expects matching numeric operands",
                )),
            },
            BinaryOp::Eq => Ok(Value::Bool(lhs == rhs)),
            BinaryOp::NotEq => Ok(Value::Bool(lhs != rhs)),
            BinaryOp::Lt => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::Bool(lhs < rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::Bool(lhs < rhs)),
                _ => Err(VmError::TypeMismatch(
                    "lt expects matching numeric operands",
                )),
            },
            BinaryOp::Gt => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::Bool(lhs > rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::Bool(lhs > rhs)),
                _ => Err(VmError::TypeMismatch(
                    "gt expects matching numeric operands",
                )),
            },
            BinaryOp::Le => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::Bool(lhs <= rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::Bool(lhs <= rhs)),
                _ => Err(VmError::TypeMismatch(
                    "le expects matching numeric operands",
                )),
            },
            BinaryOp::Ge => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::Bool(lhs >= rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::Bool(lhs >= rhs)),
                _ => Err(VmError::TypeMismatch(
                    "ge expects matching numeric operands",
                )),
            },
        }
    }
}
