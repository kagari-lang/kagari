use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::GcHeap,
    value::{Value, scalar},
};
use kagari_bytecode::instruction::{BinaryOp, UnaryOp};
use kagari_contract::{
    numeric::{NumericConversion, NumericOperation, method::IntegerMethodContract},
    representation::builtin_representation,
};
use kagari_types::{
    arithmetic::{self, ArithmeticError, IntegerBinaryOp},
    conversion, integer,
    integer::IntegerMethod,
    payload::{self, ScalarBinaryOp, operation::ScalarOperation},
    scalar::BuiltinType,
};

mod compare;

pub(crate) fn binary_operation(op: BinaryOp) -> ScalarBinaryOp {
    match op {
        BinaryOp::Add => ScalarBinaryOp::Add,
        BinaryOp::Sub => ScalarBinaryOp::Sub,
        BinaryOp::Mul => ScalarBinaryOp::Mul,
        BinaryOp::Div => ScalarBinaryOp::Div,
        BinaryOp::Rem => ScalarBinaryOp::Rem,
        BinaryOp::Eq => ScalarBinaryOp::Eq,
        BinaryOp::NotEq => ScalarBinaryOp::NotEq,
        BinaryOp::Lt => ScalarBinaryOp::Lt,
        BinaryOp::Le => ScalarBinaryOp::Le,
        BinaryOp::Gt => ScalarBinaryOp::Gt,
        BinaryOp::Ge => ScalarBinaryOp::Ge,
        BinaryOp::Numeric(_) | BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => unreachable!(),
    }
}

/// Execute a plain numeric/boolean operation without consulting the heap.
pub fn scalar_binary(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, RuntimeError> {
    match op {
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge => compare::compare(op, lhs, rhs),
        _ => binary(op, lhs, rhs),
    }
}

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

/// Admit matching Value carriers once, then use the shared typed arithmetic owner.
/// Invalid carriers precede invalid operations; arithmetic retains its checked traps.
pub fn binary(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, RuntimeError> {
    if let BinaryOp::Numeric(operation) = op {
        return fixed_integer(operation, lhs, Some(rhs));
    }
    macro_rules! integer {
        ($lhs:ident, $rhs:ident, $value:ident, $kernel:path) => {{
            let op = match op {
                BinaryOp::Add => IntegerBinaryOp::Add,
                BinaryOp::Sub => IntegerBinaryOp::Sub,
                BinaryOp::Mul => IntegerBinaryOp::Mul,
                BinaryOp::Div => IntegerBinaryOp::Div,
                BinaryOp::Rem => IntegerBinaryOp::Rem,
                _ => return Err(expected_arithmetic()),
            };
            $kernel(op, $lhs, $rhs)
                .map(Value::$value)
                .map_err(arithmetic_trap)
        }};
    }
    match (lhs, rhs) {
        (Value::I32(lhs), Value::I32(rhs)) => integer!(lhs, rhs, I32, arithmetic::i32_binary),
        (Value::I64(lhs), Value::I64(rhs)) => integer!(lhs, rhs, I64, arithmetic::i64_binary),
        (Value::U64(lhs), Value::U64(rhs)) => integer!(lhs, rhs, U64, arithmetic::u64_binary),
        (Value::F32(lhs), Value::F32(rhs)) => {
            float_bits::<true>(op, u64::from(lhs.to_bits()), u64::from(rhs.to_bits()))
                .map(|bits| Value::F32(f32::from_bits(bits as u32)))
        }
        (Value::F64(lhs), Value::F64(rhs)) => float_bits::<false>(op, lhs.to_bits(), rhs.to_bits())
            .map(|bits| Value::F64(f64::from_bits(bits))),
        _ => Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "arithmetic requires matching numeric operands",
        )),
    }
}

fn float_bits<const F32: bool>(op: BinaryOp, lhs: u64, rhs: u64) -> Result<u64, RuntimeError> {
    match op {
        BinaryOp::Add => payload::float_binary::<0, F32>(lhs, rhs),
        BinaryOp::Sub => payload::float_binary::<1, F32>(lhs, rhs),
        BinaryOp::Mul => payload::float_binary::<2, F32>(lhs, rhs),
        BinaryOp::Div => payload::float_binary::<3, F32>(lhs, rhs),
        BinaryOp::Rem => payload::float_binary::<4, F32>(lhs, rhs),
        _ => return Err(expected_arithmetic()),
    }
    .map_err(|reason| RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason))
}

#[cold]
fn expected_arithmetic() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "expected arithmetic operation",
    )
}

/// Execute the verified source-width contract, independently of Value storage width.
pub fn fixed_integer(
    operation: NumericOperation,
    lhs: Value,
    rhs: Option<Value>,
) -> Result<Value, RuntimeError> {
    let invalid = || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid numeric operand");
    operation.contract().ok_or_else(invalid)?;
    let lhs = read_integer(operation.input, &lhs)? as u64;
    let rhs = match (operation.rhs, rhs) {
        (Some(ty), Some(value)) => read_integer(ty, &value)? as u64,
        (None, None) => 0,
        _ => return Err(invalid()),
    };
    let selected = ScalarOperation::integer(operation.op, operation.input, operation.rhs)
        .ok_or_else(invalid)?;
    let bits = selected
        .evaluate(lhs, rhs)
        .map_err(|reason| RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason))?;
    Ok(scalar::decode(builtin_representation(operation.input), bits).expect("integer result"))
}

pub fn integer_method(
    heap: &GcHeap,
    operation: IntegerMethod,
    ty: BuiltinType,
    args: &[Value],
) -> Result<Option<Value>, RuntimeError> {
    let [lhs, rhs] = args else {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "integer method requires two arguments",
        ));
    };
    let (bits, signed) = ty.integer_layout().ok_or_else(|| {
        RuntimeError::new(RuntimeErrorKind::ScriptTrap, "integer receiver required")
    })?;

    let contract = IntegerMethodContract::new(operation, ty).ok_or_else(|| {
        RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "signed-offset wrapping requires an unsigned receiver",
        )
    })?;
    let (value, overflow) = integer::arithmetic_method(
        operation,
        read_integer(ty, lhs)?,
        read_integer(contract.rhs(), rhs)?,
        bits,
        signed,
    );

    let value = match ty {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => Value::I32(value as i32),
        BuiltinType::U64 => Value::U64(value as u64),
        _ => Value::I64(value as i64),
    };
    if operation.checked() && overflow {
        return Ok(None);
    }
    Ok(Some(if operation.overflowing() {
        heap.alloc_tuple(vec![value, Value::Bool(overflow)])?
    } else {
        value
    }))
}

/// Checked scalar conversion returns a Rust outcome; the installed library owns
/// its script carrier, error declaration and diagnostic construction.
pub fn checked_convert(
    source: BuiltinType,
    target: BuiltinType,
    value: Value,
) -> Result<Option<Value>, RuntimeError> {
    let fallible = conversion::checked_conversion_fallible(source, target)
        .ok_or_else(|| RuntimeError::module_validation("checked scalar conversion"))?;
    if source.integer_layout().is_some() {
        read_integer(source, &value)?;
    } else if !value.has_representation(builtin_representation(source)) {
        return Err(RuntimeError::module_validation("checked scalar source"));
    }
    if fallible {
        let input = read_integer(source, &value)?;
        let (bits, signed) = target
            .integer_layout()
            .ok_or_else(|| RuntimeError::module_validation("checked scalar target"))?;
        let (minimum, maximum) = integer::bounds(bits, signed);
        if input < minimum || input > maximum {
            return Ok(None);
        }
    }
    if source == target {
        return Ok(Some(value));
    }
    convert(NumericConversion { source, target }, value).map(Some)
}

pub fn convert(conversion: NumericConversion, value: Value) -> Result<Value, RuntimeError> {
    let fail = || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid numeric conversion");
    conversion.contract().ok_or_else(fail)?;
    if !value.has_representation(builtin_representation(conversion.source)) {
        return Err(fail());
    }
    let selected =
        ScalarOperation::conversion(conversion.source, conversion.target).ok_or_else(fail)?;
    let bits = selected
        .evaluate(scalar::encode(&value).ok_or_else(fail)?, 0)
        .map_err(|reason| RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason))?;
    scalar::decode(builtin_representation(conversion.target), bits).ok_or_else(fail)
}

pub(crate) fn read_integer(ty: BuiltinType, value: &Value) -> Result<i128, RuntimeError> {
    let fail = || {
        RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "invalid numeric operand type or range",
        )
    };
    if !value.has_representation(builtin_representation(ty)) {
        return Err(fail());
    }
    let value = match value {
        Value::I32(v) => i128::from(*v),
        Value::I64(v) => i128::from(*v),
        Value::U64(v) => i128::from(*v),
        _ => return Err(fail()),
    };
    let (bits, signed) = ty.integer_layout().ok_or_else(fail)?;
    let (minimum, maximum) = integer::bounds(bits, signed);
    if !(minimum..=maximum).contains(&value) {
        return Err(fail());
    }
    Ok(value)
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn value_arithmetic_preserves_domains_error_order_and_ieee_results() {
        for (op, expected) in [
            (BinaryOp::Add, 9),
            (BinaryOp::Sub, 3),
            (BinaryOp::Mul, 18),
            (BinaryOp::Div, 2),
            (BinaryOp::Rem, 0),
        ] {
            for (lhs, rhs, result) in [
                (Value::I32(6), Value::I32(3), Value::I32(expected)),
                (
                    Value::I64(6),
                    Value::I64(3),
                    Value::I64(i64::from(expected)),
                ),
                (Value::U64(6), Value::U64(3), Value::U64(expected as u64)),
                (
                    Value::F32(6.0),
                    Value::F32(3.0),
                    Value::F32(expected as f32),
                ),
                (
                    Value::F64(6.0),
                    Value::F64(3.0),
                    Value::F64(f64::from(expected)),
                ),
            ] {
                assert_eq!(binary(op, lhs, rhs).unwrap(), result);
            }
        }
        for (op, lhs, rhs, message) in [
            (
                BinaryOp::Add,
                Value::I32(i32::MAX),
                Value::I32(1),
                "integer overflow",
            ),
            (
                BinaryOp::Rem,
                Value::I32(i32::MIN),
                Value::I32(-1),
                "integer overflow",
            ),
            (
                BinaryOp::Div,
                Value::I64(i64::MIN),
                Value::I64(-1),
                "integer overflow",
            ),
            (
                BinaryOp::Mul,
                Value::U64(u64::MAX),
                Value::U64(2),
                "integer overflow",
            ),
            (
                BinaryOp::Div,
                Value::I64(1),
                Value::I64(0),
                "integer division by zero",
            ),
            (
                BinaryOp::Rem,
                Value::U64(1),
                Value::U64(0),
                "integer remainder by zero",
            ),
            (
                BinaryOp::Eq,
                Value::I32(1),
                Value::I64(1),
                "arithmetic requires matching numeric operands",
            ),
            (
                BinaryOp::Eq,
                Value::I32(1),
                Value::I32(1),
                "expected arithmetic operation",
            ),
        ] {
            let error = binary(op, lhs, rhs).unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            assert_eq!(error.message(), message);
        }
        let Value::F32(nan) = binary(BinaryOp::Div, Value::F32(0.0), Value::F32(0.0)).unwrap()
        else {
            panic!("f32 result")
        };
        assert!(nan.is_nan());
        let Value::F64(zero) = binary(BinaryOp::Div, Value::F64(-0.0), Value::F64(1.0)).unwrap()
        else {
            panic!("f64 result")
        };
        assert_eq!(zero.to_bits(), (-0.0f64).to_bits());
    }

    #[test]
    fn native_integer_results_and_error_context_preserve_the_declared_contract() {
        let runtime = crate::Runtime::default();
        let gc = runtime.gc();
        assert_eq!(
            integer_method(
                gc,
                IntegerMethod::WrappingAddSigned,
                BuiltinType::U64,
                &[Value::U64(0), Value::I64(-1)]
            )
            .unwrap(),
            Some(Value::U64(u64::MAX))
        );
        assert_eq!(
            integer_method(
                gc,
                IntegerMethod::RotateLeft,
                BuiltinType::U8,
                &[Value::I64(128), Value::I64(1)]
            )
            .unwrap(),
            Some(Value::I64(1))
        );
        let Value::Tuple(pair) = integer_method(
            gc,
            IntegerMethod::OverflowingAdd,
            BuiltinType::U8,
            &[Value::I64(255), Value::I64(1)],
        )
        .unwrap()
        .unwrap() else {
            panic!("overflow pair");
        };
        assert_eq!(
            &*gc.tuple(pair).unwrap(),
            &[Value::I64(0), Value::Bool(true)]
        );
        assert_eq!(
            integer_method(
                gc,
                IntegerMethod::CheckedAdd,
                BuiltinType::U8,
                &[Value::I64(255), Value::I64(1)]
            )
            .unwrap(),
            None
        );
        for (method, receiver) in [
            (IntegerMethod::WrappingAdd, BuiltinType::I8),
            (IntegerMethod::RotateRight, BuiltinType::U32),
            (IntegerMethod::WrappingAddSigned, BuiltinType::U64),
        ] {
            let error = integer_method(gc, method, receiver, &[]).unwrap_err();
            assert_eq!(error.message(), "integer method requires two arguments");
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        }
        assert_eq!(gc.active_roots(), 0);
    }

    #[rstest]
    #[case::negative_rotation(IntegerMethod::RotateLeft, BuiltinType::U8, [Value::I64(1), Value::I64(-1)])]
    #[case::rotation_above_u32(IntegerMethod::RotateRight, BuiltinType::U8, [Value::I64(1), Value::I64(4294967296)])]
    #[case::receiver_above_u8(IntegerMethod::WrappingAdd, BuiltinType::U8, [Value::I64(256), Value::I64(0)])]
    #[case::wrong_signed_carrier(IntegerMethod::WrappingAddSigned, BuiltinType::I8, [Value::I32(1), Value::I32(1)])]
    fn invalid_direct_native_inputs_return_errors_without_panicking(
        #[case] method: IntegerMethod,
        #[case] ty: BuiltinType,
        #[case] args: [Value; 2],
    ) {
        let runtime = crate::Runtime::default();
        let gc = runtime.gc();
        assert!(integer_method(gc, method, ty, &args).is_err());
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}
