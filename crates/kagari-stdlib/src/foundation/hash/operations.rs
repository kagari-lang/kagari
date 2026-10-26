//! Hash and equality callbacks run synchronously outside storage borrows.
use crate::foundation::{
    arrays::operations::{invalid as invalid_array, source as array_source},
    enumerations::value::{EnumSymbols, option},
    iteration::traversal::for_each,
};
use kagari_contract::standard::RuntimePrimitive;
use kagari_runtime::{
    error::RuntimeError,
    gc::HeapObjectId,
    native::{binding::NativeResult, context::CallContext, scalar::NativeScalar},
    value::Value,
};
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
    cx.heap().uses_builtin_keys(&cx.argument(0)?)
}

fn lookup(cx: &mut CallContext<'_>, key: &Value) -> NativeResult<(i64, i64)> {
    let collection = cx.argument(0)?;
    let _guard = cx.begin_key_lookup(0)?;
    let hash = cx.call_stored_values(0, 0, slice::from_ref(key))?;
    let Value::I64(hash) = hash.value(cx.heap()).ok_or_else(invalid)? else {
        return Err(invalid());
    };
    let mut index = 0;
    while let Some((token, stored)) = cx.heap().custom_candidate(&collection, hash, index)? {
        cx.poll()?;
        let result = cx.call_stored_values(0, 1, &[stored, *key])?;
        let Value::Bool(result) = result.value(cx.heap()).ok_or_else(invalid)? else {
            return Err(invalid());
        };
        if result {
            return Ok((hash, token));
        }
        index += 1;
    }
    Ok((hash, -1))
}

pub(crate) fn map_new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result()
}

pub(crate) fn map_len(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .map_len(map(cx)?)
        .map(NativeScalar::encode)
        .ok_or_else(invalid)
}

pub(crate) fn map_is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
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

pub(crate) fn map_get(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let value = map_value(cx)?;
    option(symbols, cx, value)
}

pub(crate) fn map_contains(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    map_value(cx).map(|value| Value::Bool(value.is_some()))
}

pub(crate) fn map_insert(cx: &mut CallContext<'_>) -> NativeResult<Value> {
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

pub(crate) fn map_insert_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    map_insert(cx)?;
    cx.argument(0)
}

pub(crate) fn map_remove(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let collection = cx.argument(0)?;
    let key = cx.argument(1)?;
    let id = map(cx)?;
    cx.ensure_collection_mutable(id)?;
    if builtin(cx)? {
        let result = option(symbols, cx, cx.heap().map_get(id, &key))?;
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
        let result = option(symbols, cx, value)?;
        if token != -1 {
            cx.heap().custom_remove(&collection, hash, token)?;
        }
        Ok(result)
    }
}

pub(crate) fn map_clear(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap().map_clear(map(cx)?)?;
    Ok(Value::Unit)
}

pub(crate) fn map_clear_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    map_clear(cx)?;
    cx.argument(0)
}

pub(crate) fn set_new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result()
}

pub(crate) fn set_len(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .set_len(set(cx)?)
        .map(NativeScalar::encode)
        .ok_or_else(invalid)
}

pub(crate) fn set_is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .set_len(set(cx)?)
        .map(|len| Value::Bool(len == 0))
        .ok_or_else(invalid)
}

pub(crate) fn set_contains(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let key = cx.argument(1)?;
    let result = if builtin(cx)? {
        cx.heap().set_contains(set(cx)?, &key).ok_or_else(invalid)?
    } else {
        lookup(cx, &key)?.1 != -1
    };
    Ok(Value::Bool(result))
}

pub(crate) fn set_insert(cx: &mut CallContext<'_>) -> NativeResult<Value> {
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

pub(crate) fn set_insert_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    set_insert(cx)?;
    cx.argument(0)
}

pub(crate) fn set_remove(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let collection = cx.argument(0)?;
    let key = cx.argument(1)?;
    let id = set(cx)?;
    cx.ensure_collection_mutable(id)?;
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

pub(crate) fn set_clear(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap().set_clear(set(cx)?)?;
    Ok(Value::Unit)
}

pub(crate) fn set_clear_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    set_clear(cx)?;
    cx.argument(0)
}

pub(crate) fn from_array(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let source = array_source(cx)?;
    let count = cx.heap().array_len(source).ok_or_else(invalid_array)?;
    let result = cx.allocate_result()?;
    let _root = cx.heap().root_value(result).ok_or_else(invalid_array)?;
    let Value::Set(target) = result else {
        return Err(invalid_array());
    };
    let hash = cx.selected_at(0)?;
    let equal = cx.selected_at(1)?;
    let builtin = hash.primitive() == Some(RuntimePrimitive::ValueHash)
        && equal.primitive() == Some(RuntimePrimitive::ValueEq);
    for index in 0..count {
        cx.poll()?;
        let value = cx
            .heap()
            .array_get(source, index)
            .ok_or_else(invalid_array)?;
        let _item = cx.heap().root_value(value).ok_or_else(invalid_array)?;
        if builtin {
            cx.heap().set_insert(target, value)?;
        } else {
            let Value::I64(code) = cx.call_values(hash, slice::from_ref(&value))? else {
                return Err(invalid_array());
            };
            let mut token = -1;
            let mut candidate = 0;
            while let Some((position, stored)) =
                cx.heap().custom_candidate(&result, code, candidate)?
            {
                cx.poll()?;
                let Value::Bool(same) = cx.call_values(equal, &[stored, value])? else {
                    return Err(invalid_array());
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

pub(crate) fn from_iter(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = cx.allocate_result()?;
    let _root = cx
        .heap()
        .root_value(result)
        .ok_or_else(invalid_construction)?;
    let insert = cx.selected_at(2)?;
    let map = matches!(result, Value::Map(_));
    for_each(symbols, cx, 3, |cx, item| {
        if map {
            let Value::Tuple(id) = item else {
                return Err(invalid_construction());
            };
            let (key, value) = {
                let tuple = cx.heap().tuple(id).ok_or_else(invalid_construction)?;
                let [key, value] = &*tuple else {
                    return Err(invalid_construction());
                };
                (*key, *value)
            };
            cx.call_values(insert, &[result, key, value])?;
        } else {
            cx.call_values(insert, &[result, item])?;
        }
        Ok(())
    })?;
    Ok(result)
}

fn invalid_construction() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}
