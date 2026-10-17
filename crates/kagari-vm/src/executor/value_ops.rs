use kagari_bytecode::instruction::{BinaryOp, UnaryOp};
use kagari_runtime::{numeric, value::Value, value_semantics};

use crate::{error::VmError, executor::Executor};

impl Executor<'_> {
    pub(crate) fn apply_unary(op: UnaryOp, value: Value) -> Result<Value, VmError> {
        numeric::unary(op, value).map_err(VmError::RuntimeError)
    }

    pub(crate) fn apply_binary(
        &self,
        op: BinaryOp,
        lhs: Value,
        rhs: Value,
    ) -> Result<Value, VmError> {
        match op {
            BinaryOp::Numeric(_)
            | BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Rem => numeric::binary(op, lhs, rhs).map_err(VmError::RuntimeError),
            BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => {
                let equal = value_semantics::identity_equal(self.runtime.gc(), &lhs, &rhs)
                    .map_err(VmError::RuntimeError)?;
                Ok(Value::Bool(if op == BinaryOp::IdentityEq {
                    equal
                } else {
                    !equal
                }))
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                let equal = value_semantics::script_equal(self.runtime.gc(), &lhs, &rhs)
                    .map_err(VmError::RuntimeError)?;
                Ok(Value::Bool(if op == BinaryOp::Eq { equal } else { !equal }))
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                numeric::scalar_binary(op, lhs, rhs).map_err(|_| {
                    VmError::TypeMismatch(match op {
                        BinaryOp::Lt => "lt expects matching numeric operands",
                        BinaryOp::Gt => "gt expects matching numeric operands",
                        BinaryOp::Le => "le expects matching numeric operands",
                        BinaryOp::Ge => "ge expects matching numeric operands",
                        _ => unreachable!(),
                    })
                })
            }
        }
    }
}
