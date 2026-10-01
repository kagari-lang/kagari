//! Closed numeric adapters derive engine constraints from actual Rust signatures.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native_module::types::TypeExpression,
    native_value::{NativeCall, NativeResult, NativeValue, invalid},
    value::Value,
};
use kagari_abi::{scalar::BuiltinType, standard::surface::StandardTypeConstraint, types::AbiType};
use std::{cmp::Ordering, marker::PhantomData};

/// A builtin number. `T` is the script type, including an erased generic
/// slot. Its signature requires the sealed `OrderedNumber` engine predicate.
/// Numeric operations reject non-finite operands without dispatching user
/// methods. Integer widths remain checked. Conversion does not reorder traps.
pub struct NativeNumber<T: NativeValue, const SIGNED: bool = false> {
    value: Value,
    ty: BuiltinType,
    _type: PhantomData<T>,
}

/// A signed integer or float with the sealed `SignedNumber` constraint.
pub type NativeSignedNumber<T> = NativeNumber<T, true>;

impl<T: NativeValue, const SIGNED: bool> NativeValue for NativeNumber<T, SIGNED> {
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        TypeExpression::Constrained {
            ty: Box::new(T::type_expression(generics)),
            constraint: if SIGNED {
                StandardTypeConstraint::SignedNumber
            } else {
                StandardTypeConstraint::OrderedNumber
            },
        }
    }

    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        let AbiType::Builtin(ty) = expected else {
            return Err(invalid());
        };
        let constraint = if SIGNED {
            StandardTypeConstraint::SignedNumber
        } else {
            StandardTypeConstraint::OrderedNumber
        };
        if !constraint.accepts_builtin_number(*ty) {
            return Err(invalid());
        }
        call.check(&value, expected)?;
        check_integer_width(&value, *ty)?;
        Ok(Self {
            value,
            ty: *ty,
            _type: PhantomData,
        })
    }

    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        if *expected != AbiType::Builtin(self.ty) {
            return Err(invalid());
        }
        call.check(&self.value, expected)?;
        check_integer_width(&self.value, self.ty)?;
        call.retain(self.value)
    }
}

impl<T: NativeValue, const SIGNED: bool> NativeNumber<T, SIGNED> {
    /// Compare compatible finite scalars without invoking script code.
    pub fn compare(&self, other: &Self) -> NativeResult<Ordering> {
        if self.ty != other.ty {
            return Err(invalid());
        }
        self.ensure_finite()?;
        other.ensure_finite()?;
        match (&self.value, &other.value) {
            (Value::I32(a), Value::I32(b)) => Ok(a.cmp(b)),
            (Value::I64(a), Value::I64(b)) => Ok(a.cmp(b)),
            (Value::U64(a), Value::U64(b)) => Ok(a.cmp(b)),
            (Value::F32(a), Value::F32(b)) => a.partial_cmp(b).ok_or_else(invalid),
            (Value::F64(a), Value::F64(b)) => a.partial_cmp(b).ok_or_else(invalid),
            _ => Err(invalid()),
        }
    }

    fn ensure_finite(&self) -> NativeResult<()> {
        if matches!(&self.value, Value::F32(x) if !x.is_finite())
            || matches!(&self.value, Value::F64(x) if !x.is_finite())
        {
            Err(trap("numeric native operand must be finite"))
        } else {
            Ok(())
        }
    }
}

impl<T: NativeValue> NativeSignedNumber<T> {
    /// Preserve the applied integer width; its minimum value traps on overflow.
    pub fn checked_abs(mut self) -> NativeResult<Self> {
        self.ensure_finite()?;
        let integer_minimum = self.ty.integer_bounds().map(|(minimum, _)| minimum);
        self.value = match self.value {
            Value::I32(x) if Some(i128::from(x)) == integer_minimum => {
                return Err(trap("signed absolute value overflow"));
            }
            Value::I64(x) if Some(i128::from(x)) == integer_minimum => {
                return Err(trap("signed absolute value overflow"));
            }
            Value::I32(x) => Value::I32(x.checked_abs().ok_or_else(invalid)?),
            Value::I64(x) => Value::I64(x.checked_abs().ok_or_else(invalid)?),
            Value::F32(x) => Value::F32(x.abs()),
            Value::F64(x) => Value::F64(x.abs()),
            _ => return Err(invalid()),
        };
        Ok(self)
    }
}

fn trap(message: &str) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::ScriptTrap, message)
}

fn check_integer_width(value: &Value, ty: BuiltinType) -> NativeResult<()> {
    let Some((minimum, maximum)) = ty.integer_bounds() else {
        return Ok(());
    };
    let integer = match value {
        Value::I32(value) => i128::from(*value),
        Value::I64(value) => i128::from(*value),
        Value::U64(value) => i128::from(*value),
        _ => return Err(invalid()),
    };
    if (minimum..=maximum).contains(&integer) {
        Ok(())
    } else {
        Err(invalid())
    }
}
