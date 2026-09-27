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
    CheckedAdd,
    CheckedSub,
    CheckedMul,
    CheckedDiv,
    CheckedRem,
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
        CheckedAdd | CheckedSub | CheckedMul | CheckedDiv | CheckedRem => {
            let method = match op {
                CheckedAdd => IntegerMethod::CheckedAdd,
                CheckedSub => IntegerMethod::CheckedSub,
                CheckedMul => IntegerMethod::CheckedMul,
                CheckedDiv => IntegerMethod::CheckedDiv,
                CheckedRem => IntegerMethod::CheckedRem,
                _ => unreachable!(),
            };
            let (value, overflow) = arithmetic_method(method, lhs, rhs, bits, signed);
            return if overflow {
                Err("integer overflow or division by zero")
            } else {
                Ok(value)
            };
        }
        BitAnd => lhs & rhs,
        BitOr => lhs | rhs,
        BitXor => lhs ^ rhs,
        BitNot => !lhs,
        Shl => lhs << rhs as u32,
        Shr => lhs >> rhs as u32,
    };
    Ok(wrap(result, bits, signed))
}

/// Explicit arithmetic policies, independent of the engine build profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntegerMethod {
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    CheckedAdd,
    CheckedSub,
    CheckedMul,
    CheckedDiv,
    CheckedRem,
    OverflowingAdd,
    OverflowingSub,
    OverflowingMul,
    SaturatingAdd,
    SaturatingSub,
    SaturatingMul,
    WrappingAddSigned,
    RotateLeft,
    RotateRight,
}

impl IntegerMethod {
    pub fn checked(self) -> bool {
        matches!(
            self,
            Self::CheckedAdd
                | Self::CheckedSub
                | Self::CheckedMul
                | Self::CheckedDiv
                | Self::CheckedRem
        )
    }
    pub fn overflowing(self) -> bool {
        matches!(
            self,
            Self::OverflowingAdd | Self::OverflowingSub | Self::OverflowingMul
        )
    }
    pub fn allocates(self) -> bool {
        self.checked() || self.overflowing()
    }
}

pub fn bounds(bits: u32, signed: bool) -> (i128, i128) {
    if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

/// Returns the policy-selected value and an overflow/undefined-operation flag.
pub fn arithmetic_method(
    op: IntegerMethod,
    lhs: i128,
    rhs: i128,
    bits: u32,
    signed: bool,
) -> (i128, bool) {
    use IntegerMethod::*;
    let (minimum, maximum) = bounds(bits, signed);
    if matches!(op, RotateLeft | RotateRight) {
        let shift = (rhs % i128::from(bits)) as u32;
        let value = wrap(lhs, bits, false);
        let result = if op == RotateLeft {
            (value << shift) | (value >> (bits - shift))
        } else {
            (value >> shift) | (value << (bits - shift))
        };
        return (wrap(result, bits, signed), false);
    }
    let exact = match op {
        WrappingAdd | CheckedAdd | OverflowingAdd | SaturatingAdd | WrappingAddSigned => {
            lhs.checked_add(rhs)
        }
        WrappingSub | CheckedSub | OverflowingSub | SaturatingSub => lhs.checked_sub(rhs),
        WrappingMul | CheckedMul | OverflowingMul | SaturatingMul => lhs.checked_mul(rhs),
        CheckedDiv => lhs.checked_div(rhs),
        CheckedRem if signed && lhs == minimum && rhs == -1 => None,
        CheckedRem => lhs.checked_rem(rhs),
        RotateLeft | RotateRight => unreachable!(),
    };
    let overflow = exact.is_none_or(|v| v < minimum || v > maximum);
    let wrapped = exact.unwrap_or_else(|| match op {
        WrappingMul | OverflowingMul | SaturatingMul => lhs.wrapping_mul(rhs),
        _ => 0,
    });
    if matches!(op, SaturatingAdd | SaturatingSub | SaturatingMul) {
        // Only unsigned 64-bit multiplication can overflow the i128 intermediate.
        return (exact.unwrap_or(maximum).clamp(minimum, maximum), overflow);
    }
    (wrap(wrapped, bits, signed), overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_policies_match_rust_at_every_width_boundary() {
        macro_rules! compare {
            ($ty:ty, $signed:expr) => {{
                let values: &[$ty] = &[<$ty>::MIN, <$ty>::MAX, 0, 1, 2, 42];
                for &lhs in values {
                    for &rhs in values {
                        let run = |op| {
                            arithmetic_method(op, lhs as i128, rhs as i128, <$ty>::BITS, $signed)
                        };
                        for (op, expected) in [
                            (IntegerMethod::WrappingAdd, lhs.wrapping_add(rhs)),
                            (IntegerMethod::WrappingSub, lhs.wrapping_sub(rhs)),
                            (IntegerMethod::WrappingMul, lhs.wrapping_mul(rhs)),
                            (IntegerMethod::SaturatingAdd, lhs.saturating_add(rhs)),
                            (IntegerMethod::SaturatingSub, lhs.saturating_sub(rhs)),
                            (IntegerMethod::SaturatingMul, lhs.saturating_mul(rhs)),
                        ] {
                            assert_eq!(
                                run(op).0,
                                expected as i128,
                                "{} {lhs} {op:?} {rhs}",
                                stringify!($ty)
                            );
                        }
                        for (op, expected) in [
                            (IntegerMethod::CheckedAdd, lhs.checked_add(rhs)),
                            (IntegerMethod::CheckedSub, lhs.checked_sub(rhs)),
                            (IntegerMethod::CheckedMul, lhs.checked_mul(rhs)),
                            (IntegerMethod::CheckedDiv, lhs.checked_div(rhs)),
                            (IntegerMethod::CheckedRem, lhs.checked_rem(rhs)),
                        ] {
                            let (value, overflow) = run(op);
                            assert_eq!(
                                (!overflow).then_some(value),
                                expected.map(|v| v as i128),
                                "{} {lhs} {op:?} {rhs}",
                                stringify!($ty)
                            );
                        }
                        for (op, expected) in [
                            (IntegerMethod::OverflowingAdd, lhs.overflowing_add(rhs)),
                            (IntegerMethod::OverflowingSub, lhs.overflowing_sub(rhs)),
                            (IntegerMethod::OverflowingMul, lhs.overflowing_mul(rhs)),
                        ] {
                            assert_eq!(run(op), (expected.0 as i128, expected.1));
                        }
                    }
                }
                for &value in values {
                    for count in [0, 1, 7, 8, 31, 32, 63, 64, u32::MAX] {
                        for (op, expected) in [
                            (IntegerMethod::RotateLeft, value.rotate_left(count)),
                            (IntegerMethod::RotateRight, value.rotate_right(count)),
                        ] {
                            assert_eq!(
                                arithmetic_method(
                                    op,
                                    value as i128,
                                    count as i128,
                                    <$ty>::BITS,
                                    $signed
                                )
                                .0,
                                expected as i128
                            );
                        }
                    }
                }
            }};
        }
        compare!(i8, true);
        compare!(i16, true);
        compare!(i32, true);
        compare!(i64, true);
        compare!(u8, false);
        compare!(u16, false);
        compare!(u32, false);
        compare!(u64, false);
    }
}
