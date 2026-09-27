//! Fixed-width integer operations shared by constant evaluation and execution.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegerOp {
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    Shl,
    Shr,
}

/// Preserve the low bits and interpret the result in the destination domain.
pub fn wrap(value: i128, bits: u32, signed: bool) -> i128 {
    let modulus = 1i128 << bits;
    let value = value & (modulus - 1);
    if signed && value >= modulus / 2 {
        value - modulus
    } else {
        value
    }
}

pub fn bit_operation(
    op: IntegerOp,
    lhs: i128,
    rhs: i128,
    bits: u32,
    signed: bool,
) -> Result<i128, &'static str> {
    use IntegerOp::*;
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err("invalid integer width");
    }
    if matches!(op, Shl | Shr) && (rhs < 0 || rhs >= i128::from(bits)) {
        return Err("integer shift out of range");
    }
    let result = match op {
        BitAnd => lhs & rhs,
        BitOr => lhs | rhs,
        BitXor => lhs ^ rhs,
        BitNot => !lhs,
        Shl => lhs << rhs as u32,
        Shr => lhs >> rhs as u32,
    };
    Ok(wrap(result, bits, signed))
}
