//! Checked numeric functions authored through the ordinary native package API.
use kagari_native_macros::native_module;

#[native_module("std::math", runtime = crate)]
pub(super) mod math {
    use crate::{
        error::{RuntimeError, RuntimeErrorKind},
        native_value::NativeResult,
    };

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
