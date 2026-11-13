#[cfg(test)]
use crate::error::RuntimeErrorKind;
use crate::{
    error::{RuntimeError, trap::TrapReason},
    value::{scalar, view::ValueView},
};

use kagari_abi::representation::ValueType;
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
    scalar::{BuiltinType, error::ScalarError},
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
pub fn scalar_binary<'call>(
    op: BinaryOp,
    lhs: ValueView<'call>,
    rhs: ValueView<'call>,
) -> Result<ValueView<'call>, RuntimeError> {
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
    scalar_trap(error.into())
}

#[cold]
fn scalar_trap(error: ScalarError) -> RuntimeError {
    RuntimeError::from(error)
}

pub fn unary<'call>(
    op: UnaryOp,
    value: ValueView<'call>,
) -> Result<ValueView<'call>, RuntimeError> {
    match (op, value.scalar_payload()) {
        (UnaryOp::Neg, Some((ValueType::I32, bits))) => arithmetic::i32_neg(bits as i32)
            .map(ValueView::from)
            .map_err(arithmetic_trap),
        (UnaryOp::Neg, Some((ValueType::I64, bits))) => arithmetic::i64_neg(bits as i64)
            .map(ValueView::from)
            .map_err(arithmetic_trap),
        (UnaryOp::Neg, Some((ValueType::F32, bits))) => {
            Ok(ValueView::from(-f32::from_bits(bits as u32)))
        }
        (UnaryOp::Neg, Some((ValueType::F64, bits))) => Ok(ValueView::from(-f64::from_bits(bits))),
        (UnaryOp::Not, Some((ValueType::Bool, bits))) => Ok(ValueView::from(bits == 0)),
        _ => Err(RuntimeError::from(TrapReason::InvalidUnaryOperand)),
    }
}

/// Admit matching inline carriers, then use the shared typed arithmetic owner.
/// The fresh scalar result borrows neither input source.
/// Invalid carriers precede invalid operations; no managed input is dereferenced.
pub fn binary<'call>(
    op: BinaryOp,
    lhs: ValueView<'_>,
    rhs: ValueView<'_>,
) -> Result<ValueView<'call>, RuntimeError> {
    if let BinaryOp::Numeric(operation) = op {
        return fixed_integer(operation, lhs, Some(rhs));
    }
    macro_rules! integer {
        ($lhs:expr, $rhs:expr, $kernel:path) => {{
            let op = match op {
                BinaryOp::Add => IntegerBinaryOp::Add,
                BinaryOp::Sub => IntegerBinaryOp::Sub,
                BinaryOp::Mul => IntegerBinaryOp::Mul,
                BinaryOp::Div => IntegerBinaryOp::Div,
                BinaryOp::Rem => IntegerBinaryOp::Rem,
                _ => return Err(expected_arithmetic()),
            };
            $kernel(op, $lhs, $rhs)
                .map(ValueView::from)
                .map_err(arithmetic_trap)
        }};
    }
    match (lhs.scalar_payload(), rhs.scalar_payload()) {
        (Some((ValueType::I32, lhs)), Some((ValueType::I32, rhs))) => {
            integer!(lhs as i32, rhs as i32, arithmetic::i32_binary)
        }
        (Some((ValueType::I64, lhs)), Some((ValueType::I64, rhs))) => {
            integer!(lhs as i64, rhs as i64, arithmetic::i64_binary)
        }
        (Some((ValueType::U64, lhs)), Some((ValueType::U64, rhs))) => {
            integer!(lhs, rhs, arithmetic::u64_binary)
        }
        (Some((ValueType::F32, lhs)), Some((ValueType::F32, rhs))) => {
            float_bits::<true>(op, lhs, rhs)
                .map(|bits| ValueView::from(f32::from_bits(bits as u32)))
        }
        (Some((ValueType::F64, lhs)), Some((ValueType::F64, rhs))) => {
            float_bits::<false>(op, lhs, rhs).map(|bits| ValueView::from(f64::from_bits(bits)))
        }
        _ => Err(RuntimeError::from(
            TrapReason::ArithmeticRequiresMatchingNumericOperands,
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
    .map_err(scalar_trap)
}

#[cold]
fn expected_arithmetic() -> RuntimeError {
    RuntimeError::from(TrapReason::ExpectedArithmeticOperation)
}

/// Execute the verified source-width contract, independently of scalar carrier width.
/// The fresh scalar result borrows neither input source.
pub fn fixed_integer<'call>(
    operation: NumericOperation,
    lhs: ValueView<'_>,
    rhs: Option<ValueView<'_>>,
) -> Result<ValueView<'call>, RuntimeError> {
    let invalid = || RuntimeError::from(TrapReason::InvalidNumericOperand);
    operation.contract().ok_or_else(invalid)?;
    let lhs = scalar::read_integer_view(operation.input, lhs)? as u64;
    let rhs = match (operation.rhs, rhs) {
        (Some(ty), Some(value)) => scalar::read_integer_view(ty, value)? as u64,
        (None, None) => 0,
        _ => return Err(invalid()),
    };
    let selected = ScalarOperation::integer(operation.op, operation.input, operation.rhs)
        .ok_or_else(invalid)?;
    let bits = selected.evaluate(lhs, rhs).map_err(scalar_trap)?;
    Ok(decode(builtin_representation(operation.input), bits).expect("integer result"))
}

/// Return scalar arithmetic and its overflow flag. The Native library owns
/// constructing an overflowing method's script tuple; this kernel allocates no
/// managed result or root. Checked overflow returns None.
pub fn integer_method<'call>(
    operation: IntegerMethod,
    ty: BuiltinType,
    args: &[ValueView<'call>],
) -> Result<Option<(ValueView<'call>, bool)>, RuntimeError> {
    let [lhs, rhs] = args else {
        return Err(RuntimeError::from(
            TrapReason::IntegerMethodRequiresTwoArguments,
        ));
    };
    let (bits, signed) = ty
        .integer_layout()
        .ok_or_else(|| RuntimeError::from(TrapReason::IntegerReceiverRequired))?;

    let contract = IntegerMethodContract::new(operation, ty).ok_or_else(|| {
        RuntimeError::from(TrapReason::SignedOffsetWrappingRequiresAnUnsignedReceiver)
    })?;
    let (value, overflow) = integer::arithmetic_method(
        operation,
        scalar::read_integer_view(ty, *lhs)?,
        scalar::read_integer_view(contract.rhs(), *rhs)?,
        bits,
        signed,
    );

    let value = match ty {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => ValueView::from(value as i32),
        BuiltinType::U64 => ValueView::from(value as u64),
        _ => ValueView::from(value as i64),
    };
    if operation.checked() && overflow {
        return Ok(None);
    }
    Ok(Some((value, overflow)))
}

/// Checked scalar conversion returns a Rust outcome; the installed library owns
/// its script carrier, error declaration and diagnostic construction.
pub fn checked_convert<'call>(
    source: BuiltinType,
    target: BuiltinType,
    value: ValueView<'call>,
) -> Result<Option<ValueView<'call>>, RuntimeError> {
    let fallible = conversion::checked_conversion_fallible(source, target)
        .ok_or_else(|| RuntimeError::module_validation("checked scalar conversion"))?;
    if source.integer_layout().is_some() {
        scalar::read_integer_view(source, value)?;
    } else if !value.has_representation(builtin_representation(source)) {
        return Err(RuntimeError::module_validation("checked scalar source"));
    }
    if fallible {
        let input = scalar::read_integer_view(source, value)?;
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

pub fn convert<'call>(
    conversion: NumericConversion,
    value: ValueView<'call>,
) -> Result<ValueView<'call>, RuntimeError> {
    let fail = || RuntimeError::from(TrapReason::InvalidNumericConversion);
    conversion.contract().ok_or_else(fail)?;
    if !value.has_representation(builtin_representation(conversion.source)) {
        return Err(fail());
    }
    let selected =
        ScalarOperation::conversion(conversion.source, conversion.target).ok_or_else(fail)?;
    let bits = selected
        .evaluate(value.scalar_payload().ok_or_else(fail)?.1, 0)
        .map_err(scalar_trap)?;
    decode(builtin_representation(conversion.target), bits).ok_or_else(fail)
}

fn decode<'call>(representation: ValueType, bits: u64) -> Option<ValueView<'call>> {
    let value = scalar::decode(representation, bits)?;
    ValueView::protected(value, None).ok()
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::native::scalar::NativeScalar;
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
                (i32::encode(6), i32::encode(3), i32::encode(expected)),
                (
                    i64::encode(6),
                    i64::encode(3),
                    i64::encode(i64::from(expected)),
                ),
                (u64::encode(6), u64::encode(3), u64::encode(expected as u64)),
                (
                    f32::encode(6.0),
                    f32::encode(3.0),
                    f32::encode(expected as f32),
                ),
                (
                    f64::encode(6.0),
                    f64::encode(3.0),
                    f64::encode(f64::from(expected)),
                ),
            ] {
                assert_eq!(
                    binary(op, lhs, rhs).unwrap().scalar_payload(),
                    result.scalar_payload()
                );
            }
        }
        for (op, lhs, rhs, message) in [
            (
                BinaryOp::Add,
                i32::encode(i32::MAX),
                i32::encode(1),
                "integer overflow",
            ),
            (
                BinaryOp::Rem,
                i32::encode(i32::MIN),
                i32::encode(-1),
                "integer overflow",
            ),
            (
                BinaryOp::Div,
                i64::encode(i64::MIN),
                i64::encode(-1),
                "integer overflow",
            ),
            (
                BinaryOp::Mul,
                u64::encode(u64::MAX),
                u64::encode(2),
                "integer overflow",
            ),
            (
                BinaryOp::Div,
                i64::encode(1),
                i64::encode(0),
                "integer division by zero",
            ),
            (
                BinaryOp::Rem,
                u64::encode(1),
                u64::encode(0),
                "integer remainder by zero",
            ),
            (
                BinaryOp::Eq,
                i32::encode(1),
                i64::encode(1),
                "arithmetic requires matching numeric operands",
            ),
            (
                BinaryOp::Eq,
                i32::encode(1),
                i32::encode(1),
                "expected arithmetic operation",
            ),
        ] {
            let error = binary(op, lhs, rhs).unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            assert_eq!(error.to_string(), message);
        }
        let nan = binary(BinaryOp::Div, f32::encode(0.0), f32::encode(0.0))
            .unwrap()
            .as_f32()
            .unwrap();
        assert!(nan.is_nan());
        let zero = binary(BinaryOp::Div, f64::encode(-0.0), f64::encode(1.0))
            .unwrap()
            .as_f64()
            .unwrap();
        assert_eq!(zero.to_bits(), (-0.0f64).to_bits());
    }

    #[test]
    fn native_integer_results_and_error_context_preserve_the_declared_contract() {
        for (operation, receiver, arguments, value, overflow) in [
            (
                IntegerMethod::WrappingAddSigned,
                BuiltinType::U64,
                [ValueView::from(0_u64), ValueView::from(-1_i64)],
                ValueView::from(u64::MAX),
                true,
            ),
            (
                IntegerMethod::RotateLeft,
                BuiltinType::U8,
                [ValueView::from(128_i64), ValueView::from(1_i64)],
                ValueView::from(1_i64),
                false,
            ),
            (
                IntegerMethod::OverflowingAdd,
                BuiltinType::U8,
                [ValueView::from(255_i64), ValueView::from(1_i64)],
                ValueView::from(0_i64),
                true,
            ),
        ] {
            let (result, actual_overflow) = integer_method(operation, receiver, &arguments)
                .unwrap()
                .unwrap();
            assert_eq!(result.scalar_payload(), value.scalar_payload());
            assert_eq!(actual_overflow, overflow);
        }
        assert!(
            integer_method(
                IntegerMethod::CheckedAdd,
                BuiltinType::U8,
                &[ValueView::from(255_i64), ValueView::from(1_i64)]
            )
            .unwrap()
            .is_none()
        );
        for (method, receiver) in [
            (IntegerMethod::WrappingAdd, BuiltinType::I8),
            (IntegerMethod::RotateRight, BuiltinType::U32),
            (IntegerMethod::WrappingAddSigned, BuiltinType::U64),
        ] {
            let error = integer_method(method, receiver, &[]).unwrap_err();
            assert_eq!(error.to_string(), "integer method requires two arguments");
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        }
    }

    #[rstest]
    #[case::negative_rotation(IntegerMethod::RotateLeft, BuiltinType::U8, [i64::encode(1), i64::encode(-1)])]
    #[case::rotation_above_u32(IntegerMethod::RotateRight, BuiltinType::U8, [i64::encode(1), i64::encode(4294967296)])]
    #[case::receiver_above_u8(IntegerMethod::WrappingAdd, BuiltinType::U8, [i64::encode(256), i64::encode(0)])]
    #[case::wrong_signed_carrier(IntegerMethod::WrappingAddSigned, BuiltinType::I8, [i32::encode(1), i32::encode(1)])]
    fn invalid_direct_native_inputs_return_errors_without_panicking(
        #[case] method: IntegerMethod,
        #[case] ty: BuiltinType,
        #[case] args: [ValueView<'static>; 2],
    ) {
        assert!(integer_method(method, ty, &args).is_err());
    }
}
