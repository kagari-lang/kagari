//! Fixed arrays expose bounded element access; constructors copy into library storage.
use kagari_contract::standard::RuntimePrimitive;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::HeapObjectId,
    native::{binding::NativeResult, context::CallContext, scalar::NativeScalar},
    value::Value,
};
use std::slice;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("builtin array contract")
}

fn source(cx: &CallContext<'_>) -> NativeResult<HeapObjectId> {
    let Value::Array(id) = cx.checked_argument(0)? else {
        return Err(invalid());
    };
    cx.heap().array_len(id).ok_or_else(invalid)?;
    Ok(id)
}

pub(super) fn length(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .array_len(source(cx)?)
        .map(NativeScalar::encode)
        .ok_or_else(invalid)
}

pub(super) fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(usize::decode(length(cx)?)? == 0))
}

pub(super) fn index(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .array_element(source(cx)?, usize::decode(cx.argument(1)?)?)?
        .ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                "array index is out of bounds",
            )
        })
}

pub(super) fn vector(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let source = source(cx)?;
    let count = cx.heap().array_len(source).ok_or_else(invalid)?;
    let result = cx.allocate_result()?;
    let _root = cx.heap().root_value(result).ok_or_else(invalid)?;
    let Value::GcHandle(target) = result else {
        return Err(invalid());
    };
    for index in 0..count {
        cx.poll()?;
        let value = cx.heap().array_get(source, index).ok_or_else(invalid)?;
        cx.heap().sequence_push(target, value)?;
    }
    Ok(result)
}

pub(super) fn set(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let source = source(cx)?;
    let _source = cx
        .heap()
        .begin_collection_iteration(&Value::Array(source))?;
    let count = cx.heap().array_len(source).ok_or_else(invalid)?;
    let result = cx.allocate_result()?;
    let _root = cx.heap().root_value(result).ok_or_else(invalid)?;
    let Value::Set(target) = result else {
        return Err(invalid());
    };
    let hash = cx.selected_at(0)?;
    let equal = cx.selected_at(1)?;
    let builtin = hash.primitive() == Some(RuntimePrimitive::ValueHash)
        && equal.primitive() == Some(RuntimePrimitive::ValueEq);
    for index in 0..count {
        cx.poll()?;
        let value = cx.heap().array_get(source, index).ok_or_else(invalid)?;
        let _item = cx.heap().root_value(value).ok_or_else(invalid)?;
        if builtin {
            cx.heap().set_insert(target, value)?;
        } else {
            let Value::I64(code) = cx.call_values(hash, slice::from_ref(&value))? else {
                return Err(invalid());
            };
            let mut token = -1;
            let mut candidate = 0;
            while let Some((position, stored)) =
                cx.heap().custom_candidate(&result, code, candidate)?
            {
                cx.poll()?;
                let Value::Bool(same) = cx.call_values(equal, &[stored, value])? else {
                    return Err(invalid());
                };
                if same {
                    token = position;
                    break;
                }
                candidate += 1;
            }
            cx.heap()
                .custom_insert(&result, code, token, value, Value::Unit)?;
        }
    }
    Ok(result)
}
