use kagari_ir::builtin::{BuiltinMethod, array};

use crate::value::Value;
use crate::{builtin::BuiltinError, gc::GcHeap};

pub fn invoke_method(
    gc: &GcHeap,
    method: BuiltinMethod,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    match method {
        BuiltinMethod::Array(array::Method::Len) => len(gc, args),
        BuiltinMethod::Array(array::Method::Push) => push(gc, args),
        BuiltinMethod::Array(array::Method::Pop) => pop(gc, args),
        _ => Err(BuiltinError::new("array builtin received non-array method")),
    }
}

fn len(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("array.len expects one array argument"));
    };

    match value {
        Value::Array(handle) => gc
            .array_len(*handle)
            .map(|len| Value::I64(len as i64))
            .ok_or_else(|| BuiltinError::new("array.len expects valid array handle")),
        _ => Err(BuiltinError::new("array.len expects array value")),
    }
}

fn push(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, next_value] = args else {
        return Err(BuiltinError::new(
            "array.push expects array and value arguments",
        ));
    };

    match value {
        Value::Array(handle) => gc
            .array_push(*handle, next_value.clone())
            .map(|_| Value::Array(*handle))
            .ok_or_else(|| BuiltinError::new("array.push expects valid array handle")),
        _ => Err(BuiltinError::new("array.push expects array value")),
    }
}

fn pop(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("array.pop expects one array argument"));
    };

    match value {
        Value::Array(handle) => {
            if gc.array_pop(*handle).is_none() {
                return Err(BuiltinError::new("array.pop expects non-empty array"));
            }
            Ok(Value::Array(*handle))
        }
        _ => Err(BuiltinError::new("array.pop expects array value")),
    }
}
