//! Shared parameterized paths for operand forms that do not duplicate handlers.
use crate::{
    integer::{self, IntegerOp},
    payload::{
        decode_integer_domain, float_binary,
        operation::{FloatOperation, FloatType, ShiftKind},
    },
    scalar::IntegerType,
};

/// Decode and validate the complete source domain before numeric evaluation.
#[inline(never)]
pub fn decode_integer(value: u64, domain: IntegerType) -> Result<i128, &'static str> {
    let (bits, signed) = domain.layout();
    decode_integer_domain(value, bits, signed)
}

/// Evaluate an integer operation without repeating interpreter operand handling.
#[inline(never)]
pub fn integer(
    op: IntegerOp,
    domain: IntegerType,
    lhs: u64,
    rhs: u64,
) -> Result<u64, &'static str> {
    let lhs = decode_integer(lhs, domain)?;
    let rhs = decode_integer(rhs, domain)?;
    let (bits, signed) = domain.layout();
    integer::integer_operation(op, lhs, rhs, bits, signed).map(|value| value as u64)
}

/// Evaluate a shift with independently validated left and count domains.
#[inline(never)]
pub fn shift(
    kind: ShiftKind,
    count_domain: IntegerType,
    lhs: u64,
    rhs: u64,
) -> Result<u64, &'static str> {
    let (domain, op) = kind.parts();
    let lhs = decode_integer(lhs, domain)?;
    let rhs = decode_integer(rhs, count_domain)?;
    let (bits, signed) = domain.layout();
    integer::integer_operation(op, lhs, rhs, bits, signed).map(|value| value as u64)
}

/// Select floating arithmetic while retaining bit-preserving payload transport.
#[inline(never)]
pub fn float(
    op: FloatOperation,
    domain: FloatType,
    lhs: u64,
    rhs: u64,
) -> Result<u64, &'static str> {
    match (domain, op) {
        (FloatType::F32, FloatOperation::Add) => float_binary::<0, true>(lhs, rhs),
        (FloatType::F32, FloatOperation::Sub) => float_binary::<1, true>(lhs, rhs),
        (FloatType::F32, FloatOperation::Mul) => float_binary::<2, true>(lhs, rhs),
        (FloatType::F32, FloatOperation::Div) => float_binary::<3, true>(lhs, rhs),
        (FloatType::F32, FloatOperation::Rem) => float_binary::<4, true>(lhs, rhs),
        (FloatType::F64, FloatOperation::Add) => float_binary::<0, false>(lhs, rhs),
        (FloatType::F64, FloatOperation::Sub) => float_binary::<1, false>(lhs, rhs),
        (FloatType::F64, FloatOperation::Mul) => float_binary::<2, false>(lhs, rhs),
        (FloatType::F64, FloatOperation::Div) => float_binary::<3, false>(lhs, rhs),
        (FloatType::F64, FloatOperation::Rem) => float_binary::<4, false>(lhs, rhs),
    }
}
