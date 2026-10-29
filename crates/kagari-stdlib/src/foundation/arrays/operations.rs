//! Fixed arrays expose bounded element access; constructors copy into library storage.
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::reference::GcRef,
    native::{binding::NativeResult, context::CallContext, scalar::NativeScalar},
    value::Value,
};

pub(crate) fn invalid() -> RuntimeError {
    RuntimeError::module_validation("builtin array contract")
}

pub(crate) fn source(cx: &CallContext<'_>) -> NativeResult<GcRef> {
    // SAFETY: The call frame retains the array throughout this operation.
    let Value::Array(id) = unsafe { cx.checked_argument_transport(0) }? else {
        return Err(invalid());
    };
    unsafe { cx.heap().array_len_transport(id) }.ok_or_else(invalid)?;
    Ok(id)
}

pub(crate) fn length(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    unsafe { cx.heap().array_len_transport(source(cx)?) }
        .map(NativeScalar::encode)
        .ok_or_else(invalid)
}

pub(crate) fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(u64::decode(length(cx)?)? == 0))
}

pub(crate) fn index(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let source = source(cx)?;
    let index = index_argument(cx, 1)?;
    let out_of_bounds = || {
        RuntimeError::new(
            RuntimeErrorKind::IndexOutOfBounds,
            format!("array index {index} is out of bounds"),
        )
    };

    unsafe { cx.heap().array_element_transport(source, index) }?.ok_or_else(out_of_bounds)
}

/// Compare fixed arrays in order through the checked element operation.
pub(crate) fn equal(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let left = source(cx)?;
    // SAFETY: The call frame retains the array throughout this operation.
    let Value::Array(right) = unsafe { cx.checked_argument_transport(1) }? else {
        return Err(invalid());
    };
    let count = unsafe { cx.heap().array_len_transport(left) }.ok_or_else(invalid)?;
    if count != unsafe { cx.heap().array_len_transport(right) }.ok_or_else(invalid)? {
        return Ok(Value::Bool(false));
    }
    let equal = cx.selected_at(0)?;
    for index in 0..count {
        cx.poll()?;
        let a = unsafe { cx.heap().array_get_transport(left, index) }.ok_or_else(invalid)?;
        let b = unsafe { cx.heap().array_get_transport(right, index) }.ok_or_else(invalid)?;
        let _a = unsafe {
            // SAFETY: Argument arrays protect extracted elements until both working roots precede the equality callback.
            cx.heap().root_value_transport(a)
        }
        .ok_or_else(invalid)?;
        let _b = unsafe {
            // SAFETY: Argument arrays protect extracted elements until both working roots precede the equality callback.
            cx.heap().root_value_transport(b)
        }
        .ok_or_else(invalid)?;
        // SAFETY: The element working roots cover the callback; its scalar result is decoded
        // immediately.
        let Value::Bool(same) = unsafe { cx.call_values_transport(equal, &[a, b]) }? else {
            return Err(invalid());
        };
        if !same {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

pub(crate) fn index_argument(cx: &CallContext<'_>, slot: usize) -> NativeResult<u64> {
    // SAFETY: Decode the scalar immediately while the argument frame is live.
    u64::decode(unsafe { cx.argument_transport(slot) }?)
}
