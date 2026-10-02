use crate::{Runtime, error::RuntimeErrorKind, value::Value};
use kagari_abi::{
    native::NativeHelperSymbol,
    native_call::{
        JIT_POLL_EXECUTION_SYMBOL, JIT_STATUS_CANCELLED, JIT_STATUS_ENGINE_FAULT,
        JIT_STATUS_INVALID_HEAP_REFERENCE, JIT_STATUS_INVALID_RUNTIME, JIT_STATUS_OK,
        JIT_STATUS_RESOURCE_LIMIT, JIT_VALUE_TAG_BOOL, JIT_VALUE_TAG_I32, JIT_VALUE_TAG_UNIT,
        JitValue,
    },
};

/// Process-lifetime symbols, independent of any runtime instance or host binding.
pub fn native_helper_symbols() -> Vec<NativeHelperSymbol> {
    vec![NativeHelperSymbol {
        symbol: JIT_POLL_EXECUTION_SYMBOL.into(),
        address: jit_poll_execution as *const () as usize,
    }]
}

/// Publish a verified program point, poll cancellation and service GC.
/// Absent frames and invalid offsets remain engine faults.
///
/// # Safety
/// A non-null pointer must reference a live Runtime for the duration of this call.
/// Generated code must obey that runtime's single-threaded execution ownership.
pub unsafe extern "C" fn jit_poll_execution(runtime: *const Runtime, offset: u64) -> i32 {
    let Some(runtime) = (unsafe { runtime.as_ref() }) else {
        return JIT_STATUS_INVALID_RUNTIME;
    };
    match usize::try_from(offset)
        .ok()
        .and_then(|offset| runtime.record_native_instruction(offset).ok())
    {
        Some(()) => {}
        None => return JIT_STATUS_ENGINE_FAULT,
    };
    if let Err(error) = runtime.gc_safepoint() {
        return match error.kind() {
            RuntimeErrorKind::EngineFault => JIT_STATUS_ENGINE_FAULT,
            RuntimeErrorKind::Cancelled => JIT_STATUS_CANCELLED,
            RuntimeErrorKind::ResourceLimitExceeded => JIT_STATUS_RESOURCE_LIMIT,
            _ => JIT_STATUS_INVALID_HEAP_REFERENCE,
        };
    }
    JIT_STATUS_OK
}

pub fn decode_native_value(value: JitValue) -> Option<Value> {
    match value.tag {
        JIT_VALUE_TAG_UNIT => Some(Value::Unit),
        JIT_VALUE_TAG_BOOL => Some(Value::Bool(value.payload != 0)),
        JIT_VALUE_TAG_I32 => i32::try_from(value.payload).ok().map(Value::I32),
        _ => None,
    }
}
