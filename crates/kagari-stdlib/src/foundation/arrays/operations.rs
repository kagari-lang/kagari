//! Fixed arrays expose bounded element access; constructors copy into library storage.
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::HeapObjectId,
    native::{binding::NativeResult, context::CallContext, scalar::NativeScalar},
    value::Value,
};

pub(crate) fn invalid() -> RuntimeError {
    RuntimeError::module_validation("builtin array contract")
}

pub(crate) fn source(cx: &CallContext<'_>) -> NativeResult<HeapObjectId> {
    let Value::Array(id) = cx.checked_argument(0)? else {
        return Err(invalid());
    };
    cx.heap().array_len(id).ok_or_else(invalid)?;
    Ok(id)
}

pub(crate) fn length(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .array_len(source(cx)?)
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

    cx.heap()
        .array_element(source, index)?
        .ok_or_else(out_of_bounds)
}

/// Compare fixed arrays in order through the checked element operation.
pub(crate) fn equal(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let left = source(cx)?;
    let Value::Array(right) = cx.checked_argument(1)? else {
        return Err(invalid());
    };
    let count = cx.heap().array_len(left).ok_or_else(invalid)?;
    if count != cx.heap().array_len(right).ok_or_else(invalid)? {
        return Ok(Value::Bool(false));
    }
    let equal = cx.selected_at(0)?;
    for index in 0..count {
        cx.poll()?;
        let a = cx.heap().array_get(left, index).ok_or_else(invalid)?;
        let b = cx.heap().array_get(right, index).ok_or_else(invalid)?;
        let _a = cx.heap().root_value(a).ok_or_else(invalid)?;
        let _b = cx.heap().root_value(b).ok_or_else(invalid)?;
        let Value::Bool(same) = cx.call_values(equal, &[a, b])? else {
            return Err(invalid());
        };
        if !same {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

pub(crate) fn index_argument(cx: &CallContext<'_>, slot: usize) -> NativeResult<u64> {
    u64::decode(cx.argument(slot)?)
}
