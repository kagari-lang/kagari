//! Fixed arrays expose bounded element access; constructors copy into library storage.
use kagari_runtime::{
    error::{RuntimeError, reason::IndexError},
    native::{binding::NativeResult, context::NativeCall, scalar::NativeScalar},
    value::view::ValueView,
};

pub(crate) fn invalid() -> RuntimeError {
    RuntimeError::module_validation("builtin array contract")
}

pub(crate) fn source<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    let source = cx.checked_argument(0)?;
    cx.array_len(&source)?;
    Ok(source)
}

pub(crate) fn length<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    let source = source(cx)?;
    Ok(cx.array_len(&source)?.encode())
}

pub(crate) fn is_empty<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    let source = source(cx)?;
    Ok((cx.array_len(&source)? == 0).encode())
}

pub(crate) fn index<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    let source = source(cx)?;
    let index = index_argument(cx, 1)?;
    cx.array_element(&source, index)?.ok_or_else(|| {
        RuntimeError::from(IndexError::Provider(format!(
            "array index {index} is out of bounds"
        )))
    })
}

/// Compare fixed arrays in order through the checked element operation.
pub(crate) fn equal<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    let left = source(cx)?;
    let right = cx.checked_argument(1)?;
    let count = cx.array_len(&left)?;
    if count != cx.array_len(&right)? {
        return Ok(false.encode());
    }
    let equal = cx.selected_at(0)?;
    for index in 0..count {
        cx.poll()?;
        let same = cx.with_temporaries(|cx| {
            let a = cx.array_element(&left, index)?.ok_or_else(invalid)?;
            let b = cx.array_element(&right, index)?.ok_or_else(invalid)?;
            bool::decode(cx.call_values(equal, &[a, b])?)
        })?;
        if !same {
            return Ok(false.encode());
        }
    }
    Ok(true.encode())
}

pub(crate) fn index_argument(cx: &mut NativeCall<'_>, slot: usize) -> NativeResult<u64> {
    u64::decode(cx.argument(slot)?)
}
