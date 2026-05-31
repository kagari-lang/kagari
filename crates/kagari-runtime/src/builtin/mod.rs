pub mod array;
pub mod string;

use kagari_ir::builtin::BuiltinMethod;

use crate::{gc::GcHeap, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinError {
    message: String,
}

impl BuiltinError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub fn invoke(gc: &GcHeap, method: BuiltinMethod, args: &[Value]) -> Result<Value, BuiltinError> {
    let spec = method.spec();

    match method {
        BuiltinMethod::Array(_) => array::invoke_method(gc, method, args).map_err(|err| {
            BuiltinError::new(format!(
                "{}.{}: {}",
                method.owner_name(),
                spec.name,
                err.message()
            ))
        }),
        BuiltinMethod::String(_) => string::invoke_method(gc, method, args).map_err(|err| {
            BuiltinError::new(format!(
                "{}.{}: {}",
                method.owner_name(),
                spec.name,
                err.message()
            ))
        }),
    }
}
