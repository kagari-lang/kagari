use crate::{RuntimeError, RuntimeErrorKind, value::Value};
use kagari_common::arithmetic::{self, ArithmeticError, IntegerBinaryOp};
use kagari_ir::bytecode::{BinaryOp, UnaryOp};
use kagari_ir::module::abi::BuiltinType;

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
    if let BinaryOp::Numeric(operation) = op {
        return fixed_integer(operation, lhs, Some(rhs));
    }
    let op = match op {
        BinaryOp::Add => IntegerBinaryOp::Add,
        BinaryOp::Sub => IntegerBinaryOp::Sub,
        BinaryOp::Mul => IntegerBinaryOp::Mul,
        BinaryOp::Div => IntegerBinaryOp::Div,
        BinaryOp::Rem => IntegerBinaryOp::Rem,
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
        (Value::U64(lhs), Value::U64(rhs)) => {
            let result = match op {
                IntegerBinaryOp::Add => lhs.checked_add(rhs),
                IntegerBinaryOp::Sub => lhs.checked_sub(rhs),
                IntegerBinaryOp::Mul => lhs.checked_mul(rhs),
                IntegerBinaryOp::Div => lhs.checked_div(rhs),
                IntegerBinaryOp::Rem => lhs.checked_rem(rhs),
            };
            Value::U64(result.ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    if rhs == 0 && matches!(op, IntegerBinaryOp::Div | IntegerBinaryOp::Rem) {
                        "integer division by zero"
                    } else {
                        "integer overflow"
                    },
                )
            })?)
        }
        (Value::F32(lhs), Value::F32(rhs)) => Value::F32(match op {
            IntegerBinaryOp::Add => lhs + rhs,
            IntegerBinaryOp::Sub => lhs - rhs,
            IntegerBinaryOp::Mul => lhs * rhs,
            IntegerBinaryOp::Div => lhs / rhs,
            IntegerBinaryOp::Rem => lhs % rhs,
        }),
        (Value::F64(lhs), Value::F64(rhs)) => Value::F64(match op {
            IntegerBinaryOp::Add => lhs + rhs,
            IntegerBinaryOp::Sub => lhs - rhs,
            IntegerBinaryOp::Mul => lhs * rhs,
            IntegerBinaryOp::Div => lhs / rhs,
            IntegerBinaryOp::Rem => lhs % rhs,
        }),
        _ => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "arithmetic requires matching numeric operands",
            ));
        }
    })
}

/// Execute the verified source-width contract, independently of Value storage width.
pub fn fixed_integer(
    operation: kagari_ir::module::numeric::NumericOperation,
    lhs: Value,
    rhs: Option<Value>,
) -> Result<Value, RuntimeError> {
    let invalid = || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid numeric operand");
    operation.contract().ok_or_else(invalid)?;
    let lhs = read_integer(operation.input, &lhs)?;
    let rhs = match (operation.rhs, rhs) {
        (Some(ty), Some(value)) => read_integer(ty, &value)?,
        (None, None) => 0,
        _ => return Err(invalid()),
    };
    let (bits, signed) = operation.input.integer_layout().ok_or_else(invalid)?;
    let result = kagari_common::integer::integer_operation(operation.op, lhs, rhs, bits, signed)
        .map_err(|reason| RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason))?;

    Ok(match operation.input {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => Value::I32(result as i32),
        BuiltinType::U64 | BuiltinType::USize => Value::U64(result as u64),
        _ => Value::I64(result as i64),
    })
}

pub fn integer_method(
    gc: &crate::gc::GcHeap,
    operation: kagari_common::integer::IntegerMethod,
    ty: kagari_ir::module::abi::BuiltinType,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    let [lhs, rhs] = args else {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "integer method requires two arguments",
        ));
    };
    let (bits, signed) = ty.integer_layout().ok_or_else(|| {
        RuntimeError::new(RuntimeErrorKind::ScriptTrap, "integer receiver required")
    })?;
    use kagari_common::integer::IntegerMethod;
    use kagari_ir::module::abi::BuiltinType;
    let rhs_ty = match operation {
        IntegerMethod::RotateLeft | IntegerMethod::RotateRight => BuiltinType::U32,
        IntegerMethod::WrappingAddSigned if signed => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "signed-offset wrapping requires an unsigned receiver",
            ));
        }
        IntegerMethod::WrappingAddSigned => match bits {
            8 => BuiltinType::I8,
            16 => BuiltinType::I16,
            32 => BuiltinType::I32,
            _ => BuiltinType::I64,
        },
        _ => ty,
    };
    let (value, overflow) = kagari_common::integer::arithmetic_method(
        operation,
        read_integer(ty, lhs)?,
        read_integer(rhs_ty, rhs)?,
        bits,
        signed,
    );

    let value = match ty {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => Value::I32(value as i32),
        BuiltinType::U64 | BuiltinType::USize => Value::U64(value as u64),
        _ => Value::I64(value as i64),
    };
    if operation.checked() {
        use crate::value::EnumTag;
        let (tag, payload) = if overflow {
            (EnumTag::OptionNone, vec![])
        } else {
            (EnumTag::OptionSome, vec![value])
        };
        return Ok(Value::Enum(gc.alloc_enum(tag, payload)?));
    }
    if operation.overflowing() {
        return Ok(Value::Tuple(vec![value, Value::Bool(overflow)]));
    }
    Ok(value)
}

pub fn convert(
    gc: &crate::gc::GcHeap,
    conversion: kagari_ir::module::numeric::NumericConversion,
    value: Value,
) -> Result<Value, RuntimeError> {
    use kagari_common::numeric::Number;
    let fail = || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid numeric conversion");
    conversion.contract().ok_or_else(fail)?;
    if !value.has_representation(
        kagari_ir::module::abi::AbiType::Builtin(conversion.source).representation(),
    ) {
        return Err(fail());
    }
    if conversion.source.integer_layout().is_some() {
        read_integer(conversion.source, &value)?;
    }
    if conversion.checked && conversion.source == conversion.target {
        return Ok(Value::Enum(
            gc.alloc_enum(crate::value::EnumTag::ResultOk, vec![value])?,
        ));
    }
    let value = match value {
        Value::Bool(v) => Number::Integer(i128::from(v)),
        Value::I32(v) => Number::Integer(i128::from(v)),
        Value::I64(v) => Number::Integer(i128::from(v)),
        Value::U64(v) => Number::Integer(i128::from(v)),
        Value::F32(v) => Number::F32(v),
        Value::F64(v) => Number::F64(v),
        _ => return Err(fail()),
    };
    if conversion.checked
        && let (Number::Integer(input), Some((bits, signed))) =
            (value, conversion.target.integer_layout())
    {
        let (min, max) = kagari_common::integer::bounds(bits, signed);
        if input < min || input > max {
            let error = Value::Enum(gc.alloc_enum(crate::value::EnumTag::TryFromIntError, vec![])?);
            let _root = gc.root_value(error.clone()).ok_or_else(fail)?;
            return Ok(Value::Enum(
                gc.alloc_enum(crate::value::EnumTag::ResultErr, vec![error])?,
            ));
        }
    }
    let value = match kagari_common::numeric::cast(
        value,
        conversion.target.number_type().ok_or_else(fail)?,
    ) {
        Number::F32(v) => Value::F32(v),
        Number::F64(v) => Value::F64(v),
        Number::Integer(v) => {
            match kagari_ir::module::abi::AbiType::Builtin(conversion.target).representation() {
                kagari_ir::module::ValueType::I32 => Value::I32(v as i32),
                kagari_ir::module::ValueType::U64 => Value::U64(v as u64),
                _ => Value::I64(v as i64),
            }
        }
    };
    if conversion.checked {
        Ok(Value::Enum(gc.alloc_enum(
            crate::value::EnumTag::ResultOk,
            vec![value],
        )?))
    } else {
        Ok(value)
    }
}

pub(crate) fn read_integer(
    ty: kagari_ir::module::abi::BuiltinType,
    value: &Value,
) -> Result<i128, RuntimeError> {
    let fail = || {
        RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "invalid numeric operand type or range",
        )
    };
    if !value.has_representation(kagari_ir::module::abi::AbiType::Builtin(ty).representation()) {
        return Err(fail());
    }
    let value = match value {
        Value::I32(v) => i128::from(*v),
        Value::I64(v) => i128::from(*v),
        Value::U64(v) => i128::from(*v),
        _ => return Err(fail()),
    };
    let (bits, signed) = ty.integer_layout().ok_or_else(fail)?;
    let (minimum, maximum) = kagari_common::integer::bounds(bits, signed);
    if !(minimum..=maximum).contains(&value) {
        return Err(fail());
    }
    Ok(value)
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn invalid_direct_native_inputs_return_errors_without_panicking() {
        use kagari_common::integer::IntegerMethod as M;
        use kagari_ir::module::abi::BuiltinType as B;
        let runtime = crate::Runtime::default();
        for (method, ty, args) in [
            (M::RotateLeft, B::U8, [Value::I64(1), Value::I64(-1)]),
            (
                M::RotateRight,
                B::U8,
                [Value::I64(1), Value::I64(4294967296)],
            ),
            (M::WrappingAdd, B::U8, [Value::I64(256), Value::I64(0)]),
            (M::WrappingAddSigned, B::I8, [Value::I32(1), Value::I32(1)]),
        ] {
            assert!(integer_method(runtime.gc(), method, ty, &args).is_err());
        }
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}
