//! Integer arithmetic shared by constant evaluation and runtime operations.
use crate::integer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{}", self.message())]
pub enum ArithmeticError {
    Overflow,
    DivisionByZero,
    RemainderByZero,
}

impl ArithmeticError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Overflow => "integer overflow",
            Self::DivisionByZero => "integer division by zero",
            Self::RemainderByZero => "integer remainder by zero",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IntegerBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

macro_rules! binary_ops {
    ($binary:ident, $ty:ty) => {
        #[inline]
        pub fn $binary(op: IntegerBinaryOp, lhs: $ty, rhs: $ty) -> Result<$ty, ArithmeticError> {
            match op {
                IntegerBinaryOp::Add => lhs.checked_add(rhs),
                IntegerBinaryOp::Sub => lhs.checked_sub(rhs),
                IntegerBinaryOp::Mul => lhs.checked_mul(rhs),
                IntegerBinaryOp::Div => {
                    if rhs == 0 {
                        return Err(ArithmeticError::DivisionByZero);
                    }
                    lhs.checked_div(rhs)
                }
                IntegerBinaryOp::Rem => {
                    if rhs == 0 {
                        return Err(ArithmeticError::RemainderByZero);
                    }
                    lhs.checked_rem(rhs)
                }
            }
            .ok_or(ArithmeticError::Overflow)
        }
    };
}

macro_rules! integer_ops {
    ($binary:ident, $neg:ident, $abs:ident, $ty:ty) => {
        binary_ops!($binary, $ty);

        pub fn $neg(value: $ty) -> Result<$ty, ArithmeticError> {
            value.checked_neg().ok_or(ArithmeticError::Overflow)
        }

        pub fn $abs(value: $ty) -> Result<$ty, ArithmeticError> {
            value.checked_abs().ok_or(ArithmeticError::Overflow)
        }
    };
}

integer_ops!(i32_binary, i32_neg, i32_abs, i32);
integer_ops!(i64_binary, i64_neg, i64_abs, i64);

integer_ops!(i8_binary, i8_neg, i8_abs, i8);
integer_ops!(i16_binary, i16_neg, i16_abs, i16);
binary_ops!(u8_binary, u8);
binary_ops!(u16_binary, u16);
binary_ops!(u32_binary, u32);
binary_ops!(u64_binary, u64);

/// Exact source-width arithmetic with native-width checks for all domains.
#[inline]
pub fn fixed_binary(
    op: IntegerBinaryOp,
    lhs: i128,
    rhs: i128,
    bits: u32,
    signed: bool,
) -> Result<i128, ArithmeticError> {
    let (min, max) = integer::bounds(bits, signed);
    if lhs < min || lhs > max || rhs < min || rhs > max {
        return Err(ArithmeticError::Overflow);
    }
    macro_rules! run {
        ($function:ident, $ty:ty) => {
            $function(op, lhs as $ty, rhs as $ty).map(i128::from)
        };
    }
    match (bits, signed) {
        (8, true) => run!(i8_binary, i8),
        (16, true) => run!(i16_binary, i16),
        (32, true) => run!(i32_binary, i32),
        (64, true) => run!(i64_binary, i64),
        (8, false) => run!(u8_binary, u8),
        (16, false) => run!(u16_binary, u16),
        (32, false) => run!(u32_binary, u32),
        (64, false) => run!(u64_binary, u64),
        _ => Err(ArithmeticError::Overflow),
    }
}
