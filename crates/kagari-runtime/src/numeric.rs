use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::values::scalar,
    gc::GcHeap,
    value::Value,
};
use kagari_bytecode::instruction::{BinaryOp, UnaryOp};
use kagari_contract::{
    numeric::{NumericConversion, NumericOperation, method::IntegerMethodContract},
    representation::builtin_representation,
};
use kagari_types::{
    arithmetic,
    arithmetic::ArithmeticError,
    conversion, integer,
    integer::IntegerMethod,
    payload::{self, ScalarBinaryOp},
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

pub fn binary(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, RuntimeError> {
    if let BinaryOp::Numeric(operation) = op {
        return fixed_integer(operation, lhs, Some(rhs));
    }
    let ty = match (&lhs, &rhs) {
        (Value::I32(_), Value::I32(_)) => BuiltinType::I32,
        (Value::I64(_), Value::I64(_)) => BuiltinType::I64,
        (Value::U64(_), Value::U64(_)) => BuiltinType::U64,
        (Value::F32(_), Value::F32(_)) => BuiltinType::F32,
        (Value::F64(_), Value::F64(_)) => BuiltinType::F64,
        _ => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "arithmetic requires matching numeric operands",
            ));
        }
    };
    if !matches!(
        op,
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem
    ) {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "expected arithmetic operation",
        ));
    }
    let kernel = payload::binary_kernel(binary_operation(op), ty).expect("numeric carrier");
    let bits = kernel(
        scalar::encode(&lhs).expect("scalar"),
        scalar::encode(&rhs).expect("scalar"),
    )
    .map_err(|reason| RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason))?;
    Ok(scalar::decode(builtin_representation(ty), bits).expect("numeric result"))
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
    let kernel = payload::integer_kernel(operation.op, operation.input, operation.rhs)
        .ok_or_else(invalid)?;
    let bits = kernel(lhs, rhs)
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
        BuiltinType::U64 | BuiltinType::USize => Value::U64(value as u64),
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
    let kernel =
        payload::conversion_kernel(conversion.source, conversion.target).ok_or_else(fail)?;
    let bits = kernel(scalar::encode(&value).ok_or_else(fail)?, 0)
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

    #[test]
    fn native_integer_results_and_error_context_preserve_the_declared_contract() {
        let runtime = crate::Runtime::default();
        let gc = runtime.gc();
        assert_eq!(
            integer_method(
                gc,
                IntegerMethod::WrappingAddSigned,
                BuiltinType::USize,
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
            (IntegerMethod::WrappingAddSigned, BuiltinType::USize),
        ] {
            let error = integer_method(gc, method, receiver, &[]).unwrap_err();
            assert_eq!(error.message(), "integer method requires two arguments");
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        }
        assert_eq!(gc.active_roots(), 0);
    }

    #[test]
    fn invalid_direct_native_inputs_return_errors_without_panicking() {
        use kagari_types::integer::IntegerMethod as M;
        use kagari_types::scalar::BuiltinType as B;
        let runtime = crate::Runtime::default();
        let gc = runtime.gc();
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
            assert!(integer_method(gc, method, ty, &args).is_err());
        }
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}
