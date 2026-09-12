use crate::{RuntimeError, RuntimeErrorKind, value::Value};
use kagari_common::arithmetic::{self, ArithmeticError, IntegerBinaryOp};
use kagari_ir::bytecode::{BinaryOp, UnaryOp};

pub fn arithmetic_trap(error: ArithmeticError) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::ScriptTrap, error.message())
}

pub fn unary(op: UnaryOp, value: Value) -> Result<Value, RuntimeError> {
    match (op, value) {
        (UnaryOp::Neg, Value::I32(value)) => arithmetic::i32_neg(value)
            .map(Value::I32)
            .map_err(arithmetic_trap),
        (UnaryOp::Neg, Value::I64(value)) => arithmetic::i64_neg(value)
            .map(Value::I64)
            .map_err(arithmetic_trap),
        (UnaryOp::Neg, Value::F32(value)) => Ok(Value::F32(-value)),
        (UnaryOp::Neg, Value::F64(value)) => Ok(Value::F64(-value)),
        (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
        _ => Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "invalid unary operand",
        )),
    }
}

pub fn binary(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, RuntimeError> {
    let op = match op {
        BinaryOp::Add => IntegerBinaryOp::Add,
        BinaryOp::Sub => IntegerBinaryOp::Sub,
        BinaryOp::Mul => IntegerBinaryOp::Mul,
        BinaryOp::Div => IntegerBinaryOp::Div,
        _ => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected arithmetic operation",
            ));
        }
    };
    Ok(match (lhs, rhs) {
        (Value::I32(lhs), Value::I32(rhs)) => {
            Value::I32(arithmetic::i32_binary(op, lhs, rhs).map_err(arithmetic_trap)?)
        }
        (Value::I64(lhs), Value::I64(rhs)) => {
            Value::I64(arithmetic::i64_binary(op, lhs, rhs).map_err(arithmetic_trap)?)
        }
        (Value::F32(lhs), Value::F32(rhs)) => Value::F32(match op {
            IntegerBinaryOp::Add => lhs + rhs,
            IntegerBinaryOp::Sub => lhs - rhs,
            IntegerBinaryOp::Mul => lhs * rhs,
            IntegerBinaryOp::Div => lhs / rhs,
        }),
        (Value::F64(lhs), Value::F64(rhs)) => Value::F64(match op {
            IntegerBinaryOp::Add => lhs + rhs,
            IntegerBinaryOp::Sub => lhs - rhs,
            IntegerBinaryOp::Mul => lhs * rhs,
            IntegerBinaryOp::Div => lhs / rhs,
        }),
        _ => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "arithmetic requires matching numeric operands",
            ));
        }
    })
}
