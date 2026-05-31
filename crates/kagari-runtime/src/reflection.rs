use crate::{gc::GcHeap, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionError {
    message: String,
}

impl ReflectionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub fn type_of(gc: &GcHeap, value: &Value) -> Value {
    let type_name = match value {
        Value::Unit => "()",
        Value::Bool(_) => "bool",
        Value::I32(_) => "i32",
        Value::I64(_) => "i64",
        Value::F32(_) => "f32",
        Value::F64(_) => "f64",
        Value::Str(_) => "String",
        Value::Tuple(_) => "tuple",
        Value::Array(_) => "array",
        Value::Struct(handle) => {
            return Value::Str(
                gc.struct_name(*handle)
                    .unwrap_or_else(|| "struct".to_owned()),
            );
        }
        Value::GcHandle(_) => "gc_handle",
        Value::HostRef(_) => "host_ref",
        Value::HostMut(_) => "host_mut",
    };

    Value::Str(type_name.to_owned())
}

pub fn get_field(gc: &GcHeap, value: &Value, field_name: &str) -> Result<Value, ReflectionError> {
    match value {
        Value::Struct(handle) => gc
            .struct_get_field(*handle, field_name)
            .ok_or_else(|| ReflectionError::new(format!("missing field `{field_name}`"))),
        _ => Err(ReflectionError::new(
            "reflect_get_field expects struct value",
        )),
    }
}

pub fn set_field(
    gc: &GcHeap,
    value: &Value,
    field_name: &str,
    next_value: Value,
) -> Result<Value, ReflectionError> {
    match value {
        Value::Struct(handle) => {
            let Some(()) = gc.struct_set_field(*handle, field_name, next_value) else {
                return Err(ReflectionError::new(format!(
                    "missing field `{field_name}`"
                )));
            };
            Ok(Value::Struct(*handle))
        }
        _ => Err(ReflectionError::new(
            "reflect_set_field expects struct value",
        )),
    }
}

pub fn set_index(
    gc: &GcHeap,
    value: &Value,
    index: &Value,
    next_value: Value,
) -> Result<Value, ReflectionError> {
    let index = match index {
        Value::I32(index) if *index >= 0 => *index as usize,
        Value::I64(index) if *index >= 0 => *index as usize,
        _ => {
            return Err(ReflectionError::new(
                "reflect_set_index expects non-negative integer index",
            ));
        }
    };

    match value {
        Value::Array(handle) => {
            let Some(()) = gc.array_set(*handle, index, next_value) else {
                return Err(ReflectionError::new(format!("invalid index `{index}`")));
            };
            Ok(Value::Array(*handle))
        }
        Value::Tuple(elements) => {
            let mut updated = elements.clone();
            let Some(slot) = updated.get_mut(index) else {
                return Err(ReflectionError::new(format!("invalid index `{index}`")));
            };
            *slot = next_value;
            Ok(Value::Tuple(updated))
        }
        _ => Err(ReflectionError::new(
            "reflect_set_index expects array or tuple value",
        )),
    }
}
