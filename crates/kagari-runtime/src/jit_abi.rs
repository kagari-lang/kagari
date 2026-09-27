use crate::RuntimeErrorKind;
use crate::{Runtime, value::Value};
use kagari_abi::native::NativeHelperSymbol;
use kagari_abi::native_call::{
    JIT_CONSUME_INSTRUCTION_STEP_SYMBOL, JIT_STATUS_CANCELLED, JIT_STATUS_ENGINE_FAULT,
    JIT_STATUS_INVALID_HEAP_REFERENCE, JIT_STATUS_INVALID_RUNTIME, JIT_STATUS_OK,
    JIT_STATUS_RESOURCE_LIMIT, JIT_VALUE_TAG_BOOL, JIT_VALUE_TAG_I32, JIT_VALUE_TAG_UNIT, JitValue,
};

/// Process-lifetime symbols, independent of any runtime instance or host binding.
pub fn native_helper_symbols() -> Vec<NativeHelperSymbol> {
    vec![NativeHelperSymbol {
        symbol: JIT_CONSUME_INSTRUCTION_STEP_SYMBOL.into(),
        address: jit_consume_instruction_step as *const () as usize,
    }]
}

/// Charge the verified logical instruction at this offset from generated code.
/// The active execution frame supplies the charge; absent frames and invalid
/// offsets are engine faults, not unmetered native execution.
///
/// # Safety
/// A non-null pointer must reference a live Runtime for the duration of this call.
/// Generated code must obey that runtime's single-threaded execution ownership.
pub unsafe extern "C" fn jit_consume_instruction_step(runtime: *const Runtime, offset: u64) -> i32 {
    let Some(runtime) = (unsafe { runtime.as_ref() }) else {
        return JIT_STATUS_INVALID_RUNTIME;
    };
    let charge = match usize::try_from(offset)
        .ok()
        .and_then(|offset| runtime.record_native_instruction(offset).ok())
    {
        Some(charge) => charge,
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
    match runtime.consume_logical_charge(charge) {
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
