use kagari_ir::builtin::{BuiltinMethod, iterable};

use crate::value::Value;
use crate::{builtin::BuiltinError, gc::GcHeap};

pub fn invoke_method(
    gc: &GcHeap,
    method: BuiltinMethod,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    match method {
        BuiltinMethod::Iterable(iterable::Method::Len) => len(gc, args),
        BuiltinMethod::Iterable(iterable::Method::Get) => get(gc, args),
        _ => Err(BuiltinError::new(
            "iterable builtin received non-iterable method",
        )),
    }
}

fn len(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("iterable.len expects one argument"));
    };

    match value {
        Value::Array(handle) => gc
            .array_len(*handle)
            .map(|len| Value::I64(len as i64))
            .ok_or_else(|| BuiltinError::new("iterable.len expects valid array handle")),
        Value::Str(value) => Ok(Value::I64(value.chars().count() as i64)),
        _ => Err(BuiltinError::new(
            "iterable.len expects array or string value",
        )),
    }
}

fn get(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, index] = args else {
        return Err(BuiltinError::new(
            "iterable.get expects value and index arguments",
        ));
    };
    let index = index_value(index)?;

    match value {
        Value::Array(handle) => gc
            .array_get(*handle, index)
            .ok_or_else(|| BuiltinError::new("iterable.get index is out of bounds")),
        Value::Str(value) => value
            .chars()
            .nth(index)
            .map(|ch| Value::Str(ch.to_string()))
            .ok_or_else(|| BuiltinError::new("iterable.get index is out of bounds")),
        _ => Err(BuiltinError::new(
            "iterable.get expects array or string value",
        )),
    }
}

fn index_value(value: &Value) -> Result<usize, BuiltinError> {
    match value {
        Value::I32(index) if *index >= 0 => Ok(*index as usize),
        Value::I64(index) if *index >= 0 => Ok(*index as usize),
        _ => Err(BuiltinError::new(
            "iterable.get expects non-negative integer index",
        )),
    }
}
