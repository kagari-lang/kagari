//! Scalar payload semantics shared by checked adapters and prepared execution.
use crate::{
    integer::{self, IntegerOp},
    numeric::{self, Number},
    scalar::{BuiltinType, IntegerType},
};

pub mod comparison;
pub mod operation;
pub mod shared;

#[derive(Debug, Clone, Copy)]
pub enum ScalarBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Execute the statically selected operation over complete scalar carriers.
/// Invalid numeric domains and arithmetic failures retain their script error reason.
#[inline]
pub fn float_binary<const OP: u8, const F32: bool>(
    lhs: u64,
    rhs: u64,
) -> Result<u64, &'static str> {
    macro_rules! evaluate {
        ($lhs:expr, $rhs:expr) => {{
            let lhs = $lhs;
            let rhs = $rhs;
            match OP {
                0 => (lhs + rhs).to_bits() as u64,
                1 => (lhs - rhs).to_bits() as u64,
                2 => (lhs * rhs).to_bits() as u64,
                3 => (lhs / rhs).to_bits() as u64,
                4 => (lhs % rhs).to_bits() as u64,
                _ => unreachable!(),
            }
        }};
    }
    Ok(if F32 {
        evaluate!(f32::from_bits(lhs as u32), f32::from_bits(rhs as u32))
    } else {
        evaluate!(f64::from_bits(lhs), f64::from_bits(rhs))
    })
}

#[inline]
fn decode_integer<const BITS: u32, const SIGNED: bool>(value: u64) -> Result<i128, &'static str> {
    decode_integer_domain(value, BITS, SIGNED)
}

#[inline]
fn decode_integer_domain(value: u64, bits: u32, signed: bool) -> Result<i128, &'static str> {
    let value = if signed {
        i128::from(value as i64)
    } else {
        i128::from(value)
    };
    let (min, max) = integer::bounds(bits, signed);
    if value < min || value > max {
        return Err("invalid numeric operand type or range");
    }
    Ok(value)
}

/// Execute the statically selected operation over complete scalar carriers.
/// Invalid numeric domains and arithmetic failures retain their script error reason.
#[inline]
pub fn integer_value<const OP: u8, const BITS: u32, const SIGNED: bool>(
    lhs: u64,
    rhs: u64,
) -> Result<u64, &'static str> {
    let lhs = decode_integer::<BITS, SIGNED>(lhs)?;
    let (op, rhs) = match OP {
        0 => (IntegerOp::CheckedAdd, decode_integer::<BITS, SIGNED>(rhs)?),
        1 => (IntegerOp::CheckedSub, decode_integer::<BITS, SIGNED>(rhs)?),
        2 => (IntegerOp::CheckedMul, decode_integer::<BITS, SIGNED>(rhs)?),
        3 => (IntegerOp::CheckedDiv, decode_integer::<BITS, SIGNED>(rhs)?),
        4 => (IntegerOp::CheckedRem, decode_integer::<BITS, SIGNED>(rhs)?),
        5 => (IntegerOp::BitAnd, decode_integer::<BITS, SIGNED>(rhs)?),
        6 => (IntegerOp::BitOr, decode_integer::<BITS, SIGNED>(rhs)?),
        7 => (IntegerOp::BitXor, decode_integer::<BITS, SIGNED>(rhs)?),
        8 => (IntegerOp::BitNot, 0),
        _ => unreachable!(),
    };
    integer::integer_operation(op, lhs, rhs, BITS, SIGNED).map(|value| value as u64)
}

/// Execute the statically selected operation over complete scalar carriers.
/// Invalid numeric domains and arithmetic failures retain their script error reason.
#[inline]
pub fn shift_value<const RIGHT: u8, const BITS: u32, const SIGNED: bool>(
    lhs: u64,
    rhs: u64,
    count_domain: IntegerType,
) -> Result<u64, &'static str> {
    let lhs = decode_integer::<BITS, SIGNED>(lhs)?;
    let rhs = shared::decode_integer(rhs, count_domain)?;
    let op = if RIGHT == 0 {
        IntegerOp::Shl
    } else {
        IntegerOp::Shr
    };
    integer::integer_operation(op, lhs, rhs, BITS, SIGNED).map(|value| value as u64)
}

/// Execute the statically selected operation over complete scalar carriers.
/// Invalid numeric domains and arithmetic failures retain their script error reason.
#[inline]
pub fn negate_integer<const BITS: u32>(value: u64, _: u64) -> Result<u64, &'static str> {
    integer_value::<1, BITS, true>(0, value)
}

/// Execute the statically selected operation over complete scalar carriers.
/// Invalid numeric domains and arithmetic failures retain their script error reason.
#[inline]
pub fn negate_float<const F32: bool>(value: u64, _: u64) -> Result<u64, &'static str> {
    Ok(value ^ if F32 { 1 << 31 } else { 1 << 63 })
}

/// Execute the statically selected operation over complete scalar carriers.
/// Invalid numeric domains and arithmetic failures retain their script error reason.
#[inline]
pub fn boolean_not(value: u64, _: u64) -> Result<u64, &'static str> {
    Ok(u64::from(value == 0))
}

/// Convert an admitted numeric pair, preserving identity bits and source validation.
/// Conversion policy is owned by numeric::cast, including saturation/truncation.
#[inline(never)]
pub fn convert(value: u64, source: BuiltinType, target: BuiltinType) -> Result<u64, &'static str> {
    let number = match source {
        BuiltinType::F32 => Number::F32(f32::from_bits(value as u32)),
        BuiltinType::F64 => Number::F64(f64::from_bits(value)),
        _ => {
            let input = if source.integer_layout().is_some_and(|(_, signed)| signed) {
                i128::from(value as i64)
            } else {
                i128::from(value)
            };
            if let Some((min, max)) = source.integer_bounds()
                && (input < min || input > max)
            {
                return Err("invalid numeric operand type or range");
            }
            Number::Integer(input)
        }
    };
    if source == target {
        return Ok(value);
    }
    Ok(
        match numeric::cast(number, target.number_type().expect("numeric target")) {
            Number::Integer(v) => v as u64,
            Number::F32(v) => u64::from(v.to_bits()),
            Number::F64(v) => v.to_bits(),
        },
    )
}

#[cfg(test)]
mod tests;
