//! Checked numeric functions authored through the ordinary native package API.
use kagari_native_macros::native_module;

#[native_module("std::math", runtime = crate)]
pub mod math {
    use crate::{
        error::{RuntimeError, RuntimeErrorKind},
        native_value::{
            NativeResult, NativeValue,
            number::{NativeNumber, NativeSignedNumber},
        },
    };
    use std::cmp::Ordering;

    /// Return the smaller finite numeric operand. Ties preserve the left operand.
    #[native]
    pub fn min<T: NativeValue>(
        lhs: NativeNumber<T>,
        rhs: NativeNumber<T>,
    ) -> NativeResult<NativeNumber<T>> {
        Ok(if lhs.compare(&rhs)? == Ordering::Greater {
            rhs
        } else {
            lhs
        })
    }

    /// Return the larger finite numeric operand. Ties preserve the left operand.
    #[native]
    pub fn max<T: NativeValue>(
        lhs: NativeNumber<T>,
        rhs: NativeNumber<T>,
    ) -> NativeResult<NativeNumber<T>> {
        Ok(if lhs.compare(&rhs)? == Ordering::Less {
            rhs
        } else {
            lhs
        })
    }

    /// Clamp a finite numeric value. Traps when the lower bound exceeds the upper bound.
    #[native]
    pub fn clamp<T: NativeValue>(
        value: NativeNumber<T>,
        min: NativeNumber<T>,
        max: NativeNumber<T>,
    ) -> NativeResult<NativeNumber<T>> {
        if min.compare(&max)? == Ordering::Greater {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "math.clamp bounds are reversed",
            ));
        }
        Ok(if value.compare(&min)? == Ordering::Less {
            min
        } else if value.compare(&max)? == Ordering::Greater {
            max
        } else {
            value
        })
    }

    /// Return a finite signed magnitude. Traps on the minimum signed integer.
    #[native]
    pub fn abs<T: NativeValue>(
        value: NativeSignedNumber<T>,
    ) -> NativeResult<NativeSignedNumber<T>> {
        value.checked_abs()
    }

    /// Round to the nearest integer, with ties away from zero. Requires finite input/result.
    #[native]
    pub fn round(value: f64) -> NativeResult<f64> {
        finite_unary(value, "math.round", f64::round)
    }

    /// Return the sine of a finite angle in radians. Requires a finite result.
    #[native]
    pub fn sin(value: f64) -> NativeResult<f64> {
        finite_unary(value, "math.sin", f64::sin)
    }

    /// Return the cosine of a finite angle in radians. Requires a finite result.
    #[native]
    pub fn cos(value: f64) -> NativeResult<f64> {
        finite_unary(value, "math.cos", f64::cos)
    }

    /// Return the tangent of a finite angle in radians. Requires a finite result.
    #[native]
    pub fn tan(value: f64) -> NativeResult<f64> {
        finite_unary(value, "math.tan", f64::tan)
    }

    /// Round toward negative infinity. Traps for a non-finite input or result.
    #[native]
    pub fn floor(value: f64) -> NativeResult<f64> {
        finite_unary(value, "math.floor", f64::floor)
    }

    /// Round toward positive infinity. Traps for a non-finite input or result.
    #[native]
    pub fn ceil(value: f64) -> NativeResult<f64> {
        finite_unary(value, "math.ceil", f64::ceil)
    }

    /// Return the square root. Traps for a negative or non-finite input.
    #[native]
    pub fn sqrt(value: f64) -> NativeResult<f64> {
        if !value.is_finite() || value < 0.0 {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "math.sqrt expects non-negative finite f64",
            ));
        }
        Ok(value.sqrt())
    }

    fn finite_unary(value: f64, name: &str, operation: fn(f64) -> f64) -> NativeResult<f64> {
        if !value.is_finite() {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                format!("{name} expects finite f64 value"),
            ));
        }
        let result = operation(value);
        if !result.is_finite() {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                format!("{name} produced non-finite f64"),
            ));
        }
        Ok(result)
    }
}
