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
            ConstantOperand::I64(value) => Value::I64(value),
            ConstantOperand::F32(value) => Value::F32(value),
            ConstantOperand::Str(value) => Value::Str(value),
        }
    }

    pub(crate) fn apply_unary(op: UnaryOp, value: Value) -> Result<Value, VmError> {
        kagari_runtime::numeric::unary(op, value).map_err(VmError::RuntimeError)
    }

    pub(crate) fn apply_binary(
        &self,
        op: BinaryOp,
        lhs: Value,
        rhs: Value,
    ) -> Result<Value, VmError> {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                kagari_runtime::numeric::binary(op, lhs, rhs).map_err(VmError::RuntimeError)
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                let equal =
                    kagari_runtime::value_semantics::script_equal(self.runtime.gc(), &lhs, &rhs)
                        .map_err(VmError::RuntimeError)?;
                Ok(Value::Bool(if op == BinaryOp::Eq { equal } else { !equal }))
            }
            BinaryOp::Lt => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::Bool(lhs < rhs)),
                (Value::I64(lhs), Value::I64(rhs)) => Ok(Value::Bool(lhs < rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::Bool(lhs < rhs)),
                (Value::F64(lhs), Value::F64(rhs)) => Ok(Value::Bool(lhs < rhs)),
                _ => Err(VmError::TypeMismatch(
                    "lt expects matching numeric operands",
                )),
            },
            BinaryOp::Gt => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::Bool(lhs > rhs)),
                (Value::I64(lhs), Value::I64(rhs)) => Ok(Value::Bool(lhs > rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::Bool(lhs > rhs)),
                (Value::F64(lhs), Value::F64(rhs)) => Ok(Value::Bool(lhs > rhs)),
                _ => Err(VmError::TypeMismatch(
                    "gt expects matching numeric operands",
                )),
            },
            BinaryOp::Le => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::Bool(lhs <= rhs)),
                (Value::I64(lhs), Value::I64(rhs)) => Ok(Value::Bool(lhs <= rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::Bool(lhs <= rhs)),
                (Value::F64(lhs), Value::F64(rhs)) => Ok(Value::Bool(lhs <= rhs)),
                _ => Err(VmError::TypeMismatch(
                    "le expects matching numeric operands",
                )),
            },
            BinaryOp::Ge => match (lhs, rhs) {
                (Value::I32(lhs), Value::I32(rhs)) => Ok(Value::Bool(lhs >= rhs)),
                (Value::I64(lhs), Value::I64(rhs)) => Ok(Value::Bool(lhs >= rhs)),
                (Value::F32(lhs), Value::F32(rhs)) => Ok(Value::Bool(lhs >= rhs)),
                (Value::F64(lhs), Value::F64(rhs)) => Ok(Value::Bool(lhs >= rhs)),
                _ => Err(VmError::TypeMismatch(
                    "ge expects matching numeric operands",
                )),
            },
        }
    }
}
