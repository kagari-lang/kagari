//! Failures shared by scalar evaluation, payload adapters and execution.
use crate::arithmetic::ArithmeticError;

/// A scalar failure without execution state, source locations or owned messages.
/// Consumers attach their diagnostic context after evaluation has stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{}", self.message())]
pub enum ScalarError {
    Arithmetic(#[from] ArithmeticError),
    InvalidOperandDomain,
    InvalidIntegerWidth,
    ShiftOutOfRange,
    InvalidUnicodeScalar,
}

impl ScalarError {
    /// Returns the diagnostic text without allocating or discarding the reason.
    pub fn message(self) -> &'static str {
        match self {
            Self::Arithmetic(error) => error.message(),
            Self::InvalidOperandDomain => "invalid numeric operand type or range",
            Self::InvalidIntegerWidth => "invalid integer width",
            Self::ShiftOutOfRange => "integer shift out of range",
            Self::InvalidUnicodeScalar => "invalid Unicode scalar operand",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn diagnostics_preserve_text_and_arithmetic_sources() {
        for (error, message) in [
            (ArithmeticError::Overflow.into(), "integer overflow"),
            (
                ArithmeticError::DivisionByZero.into(),
                "integer division by zero",
            ),
            (
                ArithmeticError::RemainderByZero.into(),
                "integer remainder by zero",
            ),
            (
                ScalarError::InvalidOperandDomain,
                "invalid numeric operand type or range",
            ),
            (ScalarError::InvalidIntegerWidth, "invalid integer width"),
            (ScalarError::ShiftOutOfRange, "integer shift out of range"),
            (
                ScalarError::InvalidUnicodeScalar,
                "invalid Unicode scalar operand",
            ),
        ] {
            assert_eq!(error.to_string(), message);
            if let ScalarError::Arithmetic(reason) = error {
                assert_eq!(
                    error.source().unwrap().downcast_ref::<ArithmeticError>(),
                    Some(&reason)
                );
            } else {
                assert!(error.source().is_none());
            }
        }
    }
}
