//! Hash and equality callbacks run synchronously outside storage borrows.
use crate::{
    error::RuntimeError,
    gc::HeapObjectId,
    native::{
        binding::NativeResult, context::CallContext, declarations::SelectedCall,
        foundation::option, scalar::NativeScalar,
    },
    value::Value,
};
use kagari_contract::standard::RuntimePrimitive;

use std::slice;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid hash container receiver or selected result")
}

fn map(cx: &CallContext<'_>) -> NativeResult<HeapObjectId> {
    let Value::Map(id) = cx.argument(0)? else {
        return Err(invalid());
    };
    Ok(id)
}

fn set(cx: &CallContext<'_>) -> NativeResult<HeapObjectId> {
    let Value::Set(id) = cx.argument(0)? else {
        return Err(invalid());
    };
    Ok(id)
}

fn builtin(cx: &CallContext<'_>) -> NativeResult<bool> {
    Ok(
        cx.selected(SelectedCall { slot: 0 })?.primitive == Some(RuntimePrimitive::ValueHash)
            && cx.selected(SelectedCall { slot: 1 })?.primitive == Some(RuntimePrimitive::ValueEq),
    )
}

fn lookup(cx: &mut CallContext<'_>, key: &Value) -> NativeResult<(i64, i64)> {
    let collection = cx.argument(0)?;
    let _guard = cx.begin_key_lookup(0)?;
    let hash_target = cx.selected(SelectedCall { slot: 0 })?;
    let hash = cx.call_values(hash_target, slice::from_ref(key))?;
    let Value::I64(hash) = hash else {
        return Err(invalid());
    };
    let equal = cx.selected(SelectedCall { slot: 1 })?;
    let mut index = 0;
    while let Some((token, stored)) = cx.heap().custom_candidate(&collection, hash, index)? {
        cx.poll()?;
        let result = cx.call_values(equal, &[stored, key.clone()])?;
        let Value::Bool(result) = result else {
            return Err(invalid());
        };
        if result {
            return Ok((hash, token));
        }
        index += 1;
    }
    Ok((hash, -1))
}

pub(super) fn map_new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result()
}

pub(super) fn map_len(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .map_len(map(cx)?)
        .map(NativeScalar::encode)
        .ok_or_else(invalid)
}

pub(super) fn map_is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .map_len(map(cx)?)
        .map(|len| Value::Bool(len == 0))
        .ok_or_else(invalid)
}

fn map_value(cx: &mut CallContext<'_>) -> NativeResult<Option<Value>> {
    let collection = cx.argument(0)?;
    let key = cx.argument(1)?;
    if builtin(cx)? {
        return Ok(cx.heap().map_get(map(cx)?, &key));
    }
    let (hash, token) = lookup(cx, &key)?;
    if token == -1 {
        return Ok(None);
    }
    cx.heap().custom_get(&collection, hash, token)
}

pub(super) fn map_get(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let value = map_value(cx)?;
    option(cx, value)
}

pub(super) fn map_contains(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    map_value(cx).map(|value| Value::Bool(value.is_some()))
}

pub(super) fn map_insert(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let collection = cx.argument(0)?;
    let key = cx.argument(1)?;
    let value = cx.argument(2)?;
    if builtin(cx)? {
        cx.heap().map_insert(map(cx)?, key, value)?;
    } else {
        let (hash, token) = lookup(cx, &key)?;
        cx.heap()
            .custom_insert(&collection, hash, token, key, value)?;
    }
    Ok(Value::Unit)
}

pub(super) fn map_insert_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    map_insert(cx)?;
    cx.argument(0)
}

pub(super) fn map_remove(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let collection = cx.argument(0)?;
    let key = cx.argument(1)?;
    let id = map(cx)?;
    cx.heap().ensure_structure_mutable(id)?;
    if builtin(cx)? {
        let result = option(cx, cx.heap().map_get(id, &key))?;
        cx.heap().map_remove(id, &key)?;
        Ok(result)
    } else {
        let (hash, token) = lookup(cx, &key)?;
        let value = if token == -1 {
            None
        } else {
            cx.heap().custom_get(&collection, hash, token)?
        };
        // Reuse the selected lookup; commit only after the Option is allocated.
        let result = option(cx, value)?;
        if token != -1 {
            cx.heap().custom_remove(&collection, hash, token)?;
        }
        Ok(result)
    }
}

pub(super) fn map_clear(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap().map_clear(map(cx)?)?;
    Ok(Value::Unit)
}

pub(super) fn map_clear_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    map_clear(cx)?;
    cx.argument(0)
}

pub(super) fn set_new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result()
}

pub(super) fn set_len(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .set_len(set(cx)?)
        .map(NativeScalar::encode)
        .ok_or_else(invalid)
}

pub(super) fn set_is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .set_len(set(cx)?)
        .map(|len| Value::Bool(len == 0))
        .ok_or_else(invalid)
}

pub(super) fn set_contains(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let key = cx.argument(1)?;
    let result = if builtin(cx)? {
        cx.heap().set_contains(set(cx)?, &key).ok_or_else(invalid)?
    } else {
        lookup(cx, &key)?.1 != -1
    };
    Ok(Value::Bool(result))
}

pub(super) fn set_insert(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let collection = cx.argument(0)?;
    let key = cx.argument(1)?;
    if builtin(cx)? {
        cx.heap().set_insert(set(cx)?, key)?;
    } else {
        let (hash, token) = lookup(cx, &key)?;
        cx.heap()
            .custom_insert(&collection, hash, token, key, Value::Unit)?;
    }
    Ok(Value::Unit)
}

pub(super) fn set_insert_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    set_insert(cx)?;
    cx.argument(0)
}

pub(super) fn set_remove(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let collection = cx.argument(0)?;
    let key = cx.argument(1)?;
    let id = set(cx)?;
    cx.heap().ensure_structure_mutable(id)?;
    let result = if builtin(cx)? {
        cx.heap().set_remove(id, &key)?
    } else {
        let (hash, token) = lookup(cx, &key)?;
        if token == -1 {
            false
        } else {
            cx.heap().custom_remove(&collection, hash, token)?;
            true
        }
    };
    Ok(Value::Bool(result))
}

pub(super) fn set_clear(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap().set_clear(set(cx)?)?;
    Ok(Value::Unit)
}

pub(super) fn set_clear_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    set_clear(cx)?;
    cx.argument(0)
}
