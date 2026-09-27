use crate::RuntimeErrorKind;
use crate::{Runtime, value::Value};
use kagari_abi::native_call::{
    JIT_STATUS_CANCELLED, JIT_STATUS_ENGINE_FAULT, JIT_STATUS_INVALID_HEAP_REFERENCE,
    JIT_STATUS_INVALID_RUNTIME, JIT_STATUS_OK, JIT_STATUS_RESOURCE_LIMIT, JIT_VALUE_TAG_BOOL,
    JIT_VALUE_TAG_I32, JIT_VALUE_TAG_UNIT, JitValue,
};

/// Charge one logical instruction from generated code.
///
/// # Safety
/// A non-null pointer must reference a live Runtime for the duration of this call.
/// Generated code must obey that runtime's single-threaded execution ownership.
pub unsafe extern "C" fn jit_consume_instruction_step(runtime: *const Runtime, offset: u64) -> i32 {
    let Some(runtime) = (unsafe { runtime.as_ref() }) else {
        return JIT_STATUS_INVALID_RUNTIME;
    };
    if usize::try_from(offset)
        .ok()
        .is_none_or(|offset| runtime.record_native_instruction(offset).is_err())
    {
        return JIT_STATUS_ENGINE_FAULT;
    }
    if let Err(error) = runtime.gc_safepoint() {
        return match error.kind() {
            RuntimeErrorKind::EngineFault => JIT_STATUS_ENGINE_FAULT,
            RuntimeErrorKind::Cancelled => JIT_STATUS_CANCELLED,
            RuntimeErrorKind::ResourceLimitExceeded => JIT_STATUS_RESOURCE_LIMIT,
            _ => JIT_STATUS_INVALID_HEAP_REFERENCE,
        };
    }
    match runtime.consume_instruction_step() {
        Ok(()) => JIT_STATUS_OK,
        Err(error) => match error.kind() {
            RuntimeErrorKind::Cancelled => JIT_STATUS_CANCELLED,
            RuntimeErrorKind::EngineFault => JIT_STATUS_ENGINE_FAULT,
            _ => JIT_STATUS_RESOURCE_LIMIT,
        },
    }
}

pub fn decode_native_value(value: JitValue) -> Option<Value> {
    match value.tag {
        JIT_VALUE_TAG_UNIT => Some(Value::Unit),
        JIT_VALUE_TAG_BOOL => Some(Value::Bool(value.payload != 0)),
        JIT_VALUE_TAG_I32 => i32::try_from(value.payload).ok().map(Value::I32),
        _ => None,
    }
}
