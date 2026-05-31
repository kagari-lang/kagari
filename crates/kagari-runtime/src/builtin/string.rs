use kagari_ir::builtin::{BuiltinMethod, StringMethod};

use crate::value::Value;
use crate::{builtin::BuiltinError, gc::GcHeap};

pub fn invoke_method(
    _gc: &GcHeap,
    method: BuiltinMethod,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    match method {
        BuiltinMethod::String(StringMethod::Len) => len(args),
        _ => Err(BuiltinError::new(
            "string builtin received non-string method",
        )),
    }
}

fn len(args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("String.len expects one string argument"));
    };

    match value {
        Value::Str(value) => Ok(Value::I64(value.len() as i64)),
        _ => Err(BuiltinError::new("String.len expects string value")),
    }
}
