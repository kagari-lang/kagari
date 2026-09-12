//! Integer arithmetic shared by constant evaluation and runtime operations.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticError {
    Overflow,
    DivisionByZero,
}

impl ArithmeticError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Overflow => "integer overflow",
            Self::DivisionByZero => "integer division by zero",
        }
    }
}
impl fmt::Display for ArithmeticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for ArithmeticError {}

#[derive(Debug, Clone, Copy)]
pub enum IntegerBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

macro_rules! integer_ops {
    ($binary:ident, $neg:ident, $abs:ident, $ty:ty) => {
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
            }
            .ok_or(ArithmeticError::Overflow)
        }
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
