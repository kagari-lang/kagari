//! Scalar comparison has no heap access or externally supplied behavior.
use crate::{error::RuntimeError, value::Value};
use kagari_bytecode::instruction::BinaryOp;

pub(super) fn compare(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, RuntimeError> {
    macro_rules! compare {
        ($a:expr, $b:expr) => {
            match op {
                BinaryOp::Eq => $a == $b,
                BinaryOp::NotEq => $a != $b,
                BinaryOp::Lt => $a < $b,
                BinaryOp::Le => $a <= $b,
                BinaryOp::Gt => $a > $b,
                BinaryOp::Ge => $a >= $b,
                _ => return Err(RuntimeError::module_validation("scalar comparison opcode")),
            }
        };
    }
    let equality = matches!(op, BinaryOp::Eq | BinaryOp::NotEq);
    Ok(Value::Bool(match (lhs, rhs) {
        (Value::Unit, Value::Unit) if equality => op == BinaryOp::Eq,
        (Value::Bool(a), Value::Bool(b)) if equality => (a == b) == (op == BinaryOp::Eq),
        (Value::I32(a), Value::I32(b)) => compare!(a, b),
        (Value::I64(a), Value::I64(b)) => compare!(a, b),
        (Value::U64(a), Value::U64(b)) => compare!(a, b),
        (Value::F32(a), Value::F32(b)) => compare!(a, b),
        (Value::F64(a), Value::F64(b)) => compare!(a, b),
        _ => {
            return Err(RuntimeError::module_validation(
                "scalar comparison operands",
            ));
        }
    }))
}
