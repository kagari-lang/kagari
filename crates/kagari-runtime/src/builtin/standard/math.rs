use crate::builtin::BuiltinError;
use crate::value::Value;
use kagari_common::arithmetic;
use std::cmp::Ordering;
pub(super) fn math_min(args: &[Value]) -> Result<Value, BuiltinError> {
    let [lhs, rhs] = args else {
        return Err(BuiltinError::new("math.min expects two values"));
    };
    ordered_pair(lhs, rhs, "math.min", |ordering| ordering <= 0)
}

pub(super) fn math_max(args: &[Value]) -> Result<Value, BuiltinError> {
    let [lhs, rhs] = args else {
        return Err(BuiltinError::new("math.max expects two values"));
    };
    ordered_pair(lhs, rhs, "math.max", |ordering| ordering >= 0)
}

pub(super) fn math_clamp(args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, min, max] = args else {
        return Err(BuiltinError::new("math.clamp expects value, min, and max"));
    };
    if compare_ordered(min, max, "math.clamp")? > 0 {
        return Err(BuiltinError::new("math.clamp expects min <= max"));
    }
    if compare_ordered(value, min, "math.clamp")? < 0 {
        return Ok(min.clone());
    }
    if compare_ordered(value, max, "math.clamp")? > 0 {
        return Ok(max.clone());
    }
    Ok(value.clone())
}

pub(super) fn math_abs(args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("math.abs expects one value"));
    };
    match value {
        Value::I32(value) => arithmetic::i32_abs(*value)
            .map(Value::I32)
            .map_err(|error| BuiltinError::new(error.message())),
        Value::I64(value) => arithmetic::i64_abs(*value)
            .map(Value::I64)
            .map_err(|error| BuiltinError::new(error.message())),
        Value::F32(value) if value.is_finite() => Ok(Value::F32(value.abs())),
        Value::F64(value) if value.is_finite() => Ok(Value::F64(value.abs())),
        _ => Err(BuiltinError::new(
            "math.abs expects finite signed numeric value",
        )),
    }
}

pub(super) fn math_sqrt(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::F64(value)] = args else {
        return Err(BuiltinError::new("math.sqrt expects one f64 value"));
    };
    if !value.is_finite() || *value < 0.0 {
        return Err(BuiltinError::new(
            "math.sqrt expects non-negative finite f64",
        ));
    }
    Ok(Value::F64(value.sqrt()))
}

pub(super) fn math_unary_f64(
    args: &[Value],
    name: &'static str,
    f: impl FnOnce(f64) -> f64,
) -> Result<Value, BuiltinError> {
    let [Value::F64(value)] = args else {
        return Err(BuiltinError::new(format!("{name} expects one f64 value")));
    };
    if !value.is_finite() {
        return Err(BuiltinError::new(format!(
            "{name} expects finite f64 value"
        )));
    }
    let result = f(*value);
    if !result.is_finite() {
        return Err(BuiltinError::new(format!("{name} produced non-finite f64")));
    }
    Ok(Value::F64(result))
}

pub(super) fn ordered_pair(
    lhs: &Value,
    rhs: &Value,
    name: &'static str,
    keep_lhs: impl FnOnce(i8) -> bool,
) -> Result<Value, BuiltinError> {
    let ordering = compare_ordered(lhs, rhs, name)?;
    if keep_lhs(ordering) {
        Ok(lhs.clone())
    } else {
        Ok(rhs.clone())
    }
}

pub(super) fn compare_ordered(
    lhs: &Value,
    rhs: &Value,
    name: &'static str,
) -> Result<i8, BuiltinError> {
    match (lhs, rhs) {
        (Value::I32(lhs), Value::I32(rhs)) => Ok(ordering_value(lhs.cmp(rhs))),
        (Value::I64(lhs), Value::I64(rhs)) => Ok(ordering_value(lhs.cmp(rhs))),
        (Value::U64(lhs), Value::U64(rhs)) => Ok(ordering_value(lhs.cmp(rhs))),
        (Value::F32(lhs), Value::F32(rhs)) if lhs.is_finite() && rhs.is_finite() => {
            compare_f64(*lhs as f64, *rhs as f64)
        }
        (Value::F64(lhs), Value::F64(rhs)) if lhs.is_finite() && rhs.is_finite() => {
            compare_f64(*lhs, *rhs)
        }
        _ => Err(BuiltinError::new(format!(
            "{name} expects same-type finite ordered numbers"
        ))),
    }
}

pub(super) fn ordering_value(ordering: Ordering) -> i8 {
    match ordering {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

pub(super) fn compare_f64(lhs: f64, rhs: f64) -> Result<i8, BuiltinError> {
    lhs.partial_cmp(&rhs)
        .map(ordering_value)
        .ok_or_else(|| BuiltinError::new("float comparison is unordered"))
}
