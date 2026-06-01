use kagari_ir::builtin::surface::StandardIntrinsic;

use crate::{
    builtin::BuiltinError,
    gc::GcHeap,
    value::{EphemeralValue, EphemeralValueId, MapKey, Value},
};

pub trait BuiltinCallbacks {
    fn call(&mut self, id: EphemeralValueId, args: &[Value]) -> Result<Value, BuiltinError>;
}

pub struct NoBuiltinCallbacks;

impl BuiltinCallbacks for NoBuiltinCallbacks {
    fn call(&mut self, _id: EphemeralValueId, _args: &[Value]) -> Result<Value, BuiltinError> {
        Err(BuiltinError::new(
            "standard helper callback is not available in this execution context",
        ))
    }
}

pub fn invoke(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    invoke_with_callbacks(gc, intrinsic, args, &mut NoBuiltinCallbacks)
}

pub fn invoke_with_callbacks(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
    callbacks: &mut dyn BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    use StandardIntrinsic::*;

    match intrinsic {
        ArrayLen => array_len(gc, args),
        ArrayIsEmpty => array_is_empty(gc, args),
        ArrayGet => array_get(gc, args),
        ArrayPush => array_push(gc, args),
        ArrayPop => array_pop(gc, args),
        ArrayInsert => array_insert(gc, args),
        ArrayRemove => array_remove(gc, args),
        ArrayClear => array_clear(gc, args),
        MapNew => map_new(gc, args),
        MapLen => map_len(gc, args),
        MapIsEmpty => map_is_empty(gc, args),
        MapContainsKey => map_contains_key(gc, args),
        MapGet => map_get(gc, args),
        MapInsert => map_insert(gc, args),
        MapRemove => map_remove(gc, args),
        MapClear => map_clear(gc, args),
        MapKeys => map_keys(gc, args),
        MapValues => map_values(gc, args),
        MapEntries => map_entries(gc, args),
        SetNew => set_new(gc, args),
        SetLen => set_len(gc, args),
        SetIsEmpty => set_is_empty(gc, args),
        SetContains => set_contains(gc, args),
        SetInsert => set_insert(gc, args),
        SetRemove => set_remove(gc, args),
        SetClear => set_clear(gc, args),
        SetToArray => set_to_array(gc, args),
        SetUnion => set_union(gc, args),
        SetIntersection => set_intersection(gc, args),
        SetDifference => set_difference(gc, args),
        StringLenBytes => string_len_bytes(args),
        StringLenChars => string_len_chars(args),
        StringIsEmpty => string_is_empty(args),
        StringConcat => string_concat(args),
        StringContains => string_contains(args),
        StringStartsWith => string_starts_with(args),
        StringEndsWith => string_ends_with(args),
        StringSlice => string_slice(gc, args),
        OptionIsSome => option_is_some(gc, args),
        OptionIsNone => option_is_none(gc, args),
        OptionUnwrapOr => option_unwrap_or(gc, args),
        OptionMap => option_map(gc, args, callbacks),
        OptionAndThen => option_and_then(gc, args, callbacks),
        ResultIsOk => result_is_ok(gc, args),
        ResultIsErr => result_is_err(gc, args),
        ResultUnwrapOr => result_unwrap_or(gc, args),
        ResultMap => result_map(gc, args, callbacks),
        ResultMapErr => result_map_err(gc, args, callbacks),
        ResultAndThen => result_and_then(gc, args, callbacks),
        IterLen => iter_len(gc, args),
        IterIsEmpty => iter_is_empty(gc, args),
        IterGet => iter_get(gc, args),
        IterToArray => iter_to_array(gc, args),
        IterForEach => iter_for_each(gc, args, callbacks),
        MathMin => math_min(args),
        MathMax => math_max(args),
        MathClamp => math_clamp(args),
        MathAbs => math_abs(args),
        MathFloor => math_unary_f64(args, "math.floor", f64::floor),
        MathCeil => math_unary_f64(args, "math.ceil", f64::ceil),
        MathRound => math_unary_f64(args, "math.round", f64::round),
        MathSqrt => math_sqrt(args),
        MathSin => math_unary_f64(args, "math.sin", f64::sin),
        MathCos => math_unary_f64(args, "math.cos", f64::cos),
        MathTan => math_unary_f64(args, "math.tan", f64::tan),
        DebugPrint => debug_print(args),
        DebugAssert => debug_assert(args),
        DebugAssertEq => debug_assert_eq(args),
        DebugPanic => debug_panic(args),
    }
}

fn array_len(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_array(args, "array.len")?;
    gc.array_len(handle)
        .map(usize_value)
        .ok_or_else(|| BuiltinError::new("array.len expects valid array handle"))
}

fn array_is_empty(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_array(args, "array.is_empty")?;
    gc.array_len(handle)
        .map(|len| Value::Bool(len == 0))
        .ok_or_else(|| BuiltinError::new("array.is_empty expects valid array handle"))
}

fn array_get(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Array(handle), index] = args else {
        return Err(BuiltinError::new("array.get expects array and index"));
    };
    let index = index_value(index, "array.get")?;
    match gc.array_get(*handle, index) {
        Some(value) => option_some(gc, value),
        None => option_none(gc),
    }
}

fn array_push(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Array(handle), item] = args else {
        return Err(BuiltinError::new("array.push expects array and item"));
    };
    gc.array_push(*handle, item.clone())
        .map(|_| Value::Array(*handle))
        .ok_or_else(|| BuiltinError::new("array.push expects valid array and storable item"))
}

fn array_pop(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_array(args, "array.pop")?;
    match gc.array_pop(handle) {
        Some(value) => option_some(gc, value),
        None => option_none(gc),
    }
}

fn array_insert(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Array(handle), index, item] = args else {
        return Err(BuiltinError::new(
            "array.insert expects array, index, and item",
        ));
    };
    let index = index_value(index, "array.insert")?;
    gc.array_insert(*handle, index, item.clone())
        .map(|_| Value::Array(*handle))
        .ok_or_else(|| BuiltinError::new("array.insert expects valid index and storable item"))
}

fn array_remove(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Array(handle), index] = args else {
        return Err(BuiltinError::new("array.remove expects array and index"));
    };
    let index = index_value(index, "array.remove")?;
    match gc.array_remove(*handle, index) {
        Some(value) => option_some(gc, value),
        None => option_none(gc),
    }
}

fn array_clear(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_array(args, "array.clear")?;
    gc.array_clear(handle)
        .map(|_| Value::Array(handle))
        .ok_or_else(|| BuiltinError::new("array.clear expects valid array handle"))
}

fn map_new(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    if !args.is_empty() {
        return Err(BuiltinError::new("map.new expects no arguments"));
    }
    gc.alloc_map(Vec::new())
        .map(Value::Map)
        .ok_or_else(|| BuiltinError::new("map.new could not allocate map"))
}

fn map_len(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_map(args, "map.len")?;
    gc.map_len(handle)
        .map(usize_value)
        .ok_or_else(|| BuiltinError::new("map.len expects valid map handle"))
}

fn map_is_empty(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_map(args, "map.is_empty")?;
    gc.map_len(handle)
        .map(|len| Value::Bool(len == 0))
        .ok_or_else(|| BuiltinError::new("map.is_empty expects valid map handle"))
}

fn map_contains_key(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Map(handle), key] = args else {
        return Err(BuiltinError::new("map.contains_key expects map and key"));
    };
    require_hash_key(key, "map.contains_key")?;
    gc.map_len(*handle)
        .ok_or_else(|| BuiltinError::new("map.contains_key expects valid map handle"))?;
    Ok(Value::Bool(gc.map_get(*handle, key).is_some()))
}

fn map_get(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Map(handle), key] = args else {
        return Err(BuiltinError::new("map.get expects map and key"));
    };
    require_hash_key(key, "map.get")?;
    match gc.map_get(*handle, key) {
        Some(value) => option_some(gc, value),
        None => option_none(gc),
    }
}

fn map_insert(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Map(handle), key, item] = args else {
        return Err(BuiltinError::new("map.insert expects map, key, and item"));
    };
    require_hash_key(key, "map.insert")?;
    gc.map_insert(*handle, key.clone(), item.clone())
        .map(|_| Value::Map(*handle))
        .ok_or_else(|| BuiltinError::new("map.insert expects valid map and storable item"))
}

fn map_remove(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Map(handle), key] = args else {
        return Err(BuiltinError::new("map.remove expects map and key"));
    };
    require_hash_key(key, "map.remove")?;
    match gc.map_remove(*handle, key) {
        Some(value) => option_some(gc, value),
        None => option_none(gc),
    }
}

fn map_clear(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_map(args, "map.clear")?;
    gc.map_clear(handle)
        .map(|_| Value::Map(handle))
        .ok_or_else(|| BuiltinError::new("map.clear expects valid map handle"))
}

fn map_keys(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_map(args, "map.keys")?;
    let keys = gc
        .map_snapshot(handle)
        .ok_or_else(|| BuiltinError::new("map.keys expects valid map handle"))?
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    array_value(gc, keys)
}

fn map_values(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_map(args, "map.values")?;
    let values = gc
        .map_snapshot(handle)
        .ok_or_else(|| BuiltinError::new("map.values expects valid map handle"))?
        .into_iter()
        .map(|(_, value)| value)
        .collect();
    array_value(gc, values)
}

fn map_entries(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_map(args, "map.entries")?;
    let entries = gc
        .map_snapshot(handle)
        .ok_or_else(|| BuiltinError::new("map.entries expects valid map handle"))?
        .into_iter()
        .map(|(key, value)| Value::Tuple(vec![key, value]))
        .collect();
    array_value(gc, entries)
}

fn set_new(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    if !args.is_empty() {
        return Err(BuiltinError::new("set.new expects no arguments"));
    }
    gc.alloc_set(Vec::new())
        .map(Value::Set)
        .ok_or_else(|| BuiltinError::new("set.new could not allocate set"))
}

fn set_len(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_set(args, "set.len")?;
    gc.set_len(handle)
        .map(usize_value)
        .ok_or_else(|| BuiltinError::new("set.len expects valid set handle"))
}

fn set_is_empty(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_set(args, "set.is_empty")?;
    gc.set_len(handle)
        .map(|len| Value::Bool(len == 0))
        .ok_or_else(|| BuiltinError::new("set.is_empty expects valid set handle"))
}

fn set_contains(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Set(handle), item] = args else {
        return Err(BuiltinError::new("set.contains expects set and item"));
    };
    require_hash_key(item, "set.contains")?;
    gc.set_contains(*handle, item)
        .map(Value::Bool)
        .ok_or_else(|| BuiltinError::new("set.contains expects valid set handle"))
}

fn set_insert(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Set(handle), item] = args else {
        return Err(BuiltinError::new("set.insert expects set and item"));
    };
    require_hash_key(item, "set.insert")?;
    gc.set_insert(*handle, item.clone())
        .map(|_| Value::Set(*handle))
        .ok_or_else(|| BuiltinError::new("set.insert expects valid set and hash-key item"))
}

fn set_remove(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Set(handle), item] = args else {
        return Err(BuiltinError::new("set.remove expects set and item"));
    };
    require_hash_key(item, "set.remove")?;
    gc.set_remove(*handle, item)
        .map(Value::Bool)
        .ok_or_else(|| BuiltinError::new("set.remove expects valid set handle"))
}

fn set_clear(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_set(args, "set.clear")?;
    gc.set_clear(handle)
        .map(|_| Value::Set(handle))
        .ok_or_else(|| BuiltinError::new("set.clear expects valid set handle"))
}

fn set_to_array(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_set(args, "set.to_array")?;
    let values = gc
        .set_snapshot(handle)
        .ok_or_else(|| BuiltinError::new("set.to_array expects valid set handle"))?;
    array_value(gc, values)
}

fn set_union(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let (lhs, rhs) = two_sets(args, "set.union")?;
    let mut values = gc
        .set_snapshot(lhs)
        .ok_or_else(|| BuiltinError::new("set.union expects valid lhs set"))?;
    for value in gc
        .set_snapshot(rhs)
        .ok_or_else(|| BuiltinError::new("set.union expects valid rhs set"))?
    {
        if !values.iter().any(|existing| existing == &value) {
            values.push(value);
        }
    }
    set_value(gc, values)
}

fn set_intersection(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let (lhs, rhs) = two_sets(args, "set.intersection")?;
    let rhs_values = gc
        .set_snapshot(rhs)
        .ok_or_else(|| BuiltinError::new("set.intersection expects valid rhs set"))?;
    let values = gc
        .set_snapshot(lhs)
        .ok_or_else(|| BuiltinError::new("set.intersection expects valid lhs set"))?
        .into_iter()
        .filter(|value| rhs_values.iter().any(|rhs_value| rhs_value == value))
        .collect();
    set_value(gc, values)
}

fn set_difference(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let (lhs, rhs) = two_sets(args, "set.difference")?;
    let rhs_values = gc
        .set_snapshot(rhs)
        .ok_or_else(|| BuiltinError::new("set.difference expects valid rhs set"))?;
    let values = gc
        .set_snapshot(lhs)
        .ok_or_else(|| BuiltinError::new("set.difference expects valid lhs set"))?
        .into_iter()
        .filter(|value| !rhs_values.iter().any(|rhs_value| rhs_value == value))
        .collect();
    set_value(gc, values)
}

fn string_len_bytes(args: &[Value]) -> Result<Value, BuiltinError> {
    let value = one_string(args, "string.len_bytes")?;
    Ok(usize_value(value.len()))
}

fn string_len_chars(args: &[Value]) -> Result<Value, BuiltinError> {
    let value = one_string(args, "string.len_chars")?;
    Ok(usize_value(value.chars().count()))
}

fn string_is_empty(args: &[Value]) -> Result<Value, BuiltinError> {
    let value = one_string(args, "string.is_empty")?;
    Ok(Value::Bool(value.is_empty()))
}

fn string_concat(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(lhs), Value::Str(rhs)] = args else {
        return Err(BuiltinError::new("string.concat expects two strings"));
    };
    Ok(Value::Str(format!("{lhs}{rhs}")))
}

fn string_contains(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(value), Value::Str(needle)] = args else {
        return Err(BuiltinError::new("string.contains expects two strings"));
    };
    Ok(Value::Bool(value.contains(needle)))
}

fn string_starts_with(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(value), Value::Str(prefix)] = args else {
        return Err(BuiltinError::new("string.starts_with expects two strings"));
    };
    Ok(Value::Bool(value.starts_with(prefix)))
}

fn string_ends_with(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(value), Value::Str(suffix)] = args else {
        return Err(BuiltinError::new("string.ends_with expects two strings"));
    };
    Ok(Value::Bool(value.ends_with(suffix)))
}

fn string_slice(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(value), start, end] = args else {
        return Err(BuiltinError::new(
            "string.slice expects string, start, and end",
        ));
    };
    let start = index_value(start, "string.slice")?;
    let end = index_value(end, "string.slice")?;
    if start > end {
        return option_none(gc);
    }
    match value.get(start..end) {
        Some(slice) => option_some(gc, Value::Str(slice.to_owned())),
        None => option_none(gc),
    }
}

fn option_is_some(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let option = option_value(gc, args, "option.is_some")?;
    Ok(Value::Bool(option.variant == "Some"))
}

fn option_is_none(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let option = option_value(gc, args, "option.is_none")?;
    Ok(Value::Bool(option.variant == "None"))
}

fn option_unwrap_or(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, fallback] = args else {
        return Err(BuiltinError::new(
            "option.unwrap_or expects option and fallback",
        ));
    };
    match option_snapshot(gc, value, "option.unwrap_or")?
        .variant
        .as_str()
    {
        "Some" => option_payload(gc, value, "option.unwrap_or"),
        "None" => Ok(fallback.clone()),
        _ => unreachable!(),
    }
}

fn option_map(
    gc: &GcHeap,
    args: &[Value],
    callbacks: &mut dyn BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    let [value, mapper] = args else {
        return Err(BuiltinError::new("option.map expects option and mapper"));
    };
    let callback = callback_id(mapper, "option.map")?;
    match option_snapshot(gc, value, "option.map")?.variant.as_str() {
        "Some" => {
            let next = callbacks.call(callback, &[option_payload(gc, value, "option.map")?])?;
            option_some(gc, next)
        }
        "None" => option_none(gc),
        _ => unreachable!(),
    }
}

fn option_and_then(
    gc: &GcHeap,
    args: &[Value],
    callbacks: &mut dyn BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    let [value, mapper] = args else {
        return Err(BuiltinError::new(
            "option.and_then expects option and mapper",
        ));
    };
    let callback = callback_id(mapper, "option.and_then")?;
    match option_snapshot(gc, value, "option.and_then")?
        .variant
        .as_str()
    {
        "Some" => {
            let next =
                callbacks.call(callback, &[option_payload(gc, value, "option.and_then")?])?;
            option_snapshot(gc, &next, "option.and_then mapper result")?;
            Ok(next)
        }
        "None" => option_none(gc),
        _ => unreachable!(),
    }
}

fn result_is_ok(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let result = result_value(gc, args, "result.is_ok")?;
    Ok(Value::Bool(result.variant == "Ok"))
}

fn result_is_err(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let result = result_value(gc, args, "result.is_err")?;
    Ok(Value::Bool(result.variant == "Err"))
}

fn result_unwrap_or(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, fallback] = args else {
        return Err(BuiltinError::new(
            "result.unwrap_or expects result and fallback",
        ));
    };
    match result_snapshot(gc, value, "result.unwrap_or")?
        .variant
        .as_str()
    {
        "Ok" => result_payload(gc, value, "result.unwrap_or"),
        "Err" => Ok(fallback.clone()),
        _ => unreachable!(),
    }
}

fn result_map(
    gc: &GcHeap,
    args: &[Value],
    callbacks: &mut dyn BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    let [value, mapper] = args else {
        return Err(BuiltinError::new("result.map expects result and mapper"));
    };
    let callback = callback_id(mapper, "result.map")?;
    match result_snapshot(gc, value, "result.map")?.variant.as_str() {
        "Ok" => {
            let next = callbacks.call(callback, &[result_payload(gc, value, "result.map")?])?;
            result_ok(gc, next)
        }
        "Err" => result_err(gc, result_payload(gc, value, "result.map")?),
        _ => unreachable!(),
    }
}

fn result_map_err(
    gc: &GcHeap,
    args: &[Value],
    callbacks: &mut dyn BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    let [value, mapper] = args else {
        return Err(BuiltinError::new(
            "result.map_err expects result and mapper",
        ));
    };
    let callback = callback_id(mapper, "result.map_err")?;
    match result_snapshot(gc, value, "result.map_err")?
        .variant
        .as_str()
    {
        "Ok" => result_ok(gc, result_payload(gc, value, "result.map_err")?),
        "Err" => {
            let next = callbacks.call(callback, &[result_payload(gc, value, "result.map_err")?])?;
            result_err(gc, next)
        }
        _ => unreachable!(),
    }
}

fn result_and_then(
    gc: &GcHeap,
    args: &[Value],
    callbacks: &mut dyn BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    let [value, mapper] = args else {
        return Err(BuiltinError::new(
            "result.and_then expects result and mapper",
        ));
    };
    let callback = callback_id(mapper, "result.and_then")?;
    match result_snapshot(gc, value, "result.and_then")?
        .variant
        .as_str()
    {
        "Ok" => {
            let next =
                callbacks.call(callback, &[result_payload(gc, value, "result.and_then")?])?;
            result_snapshot(gc, &next, "result.and_then mapper result")?;
            Ok(next)
        }
        "Err" => result_err(gc, result_payload(gc, value, "result.and_then")?),
        _ => unreachable!(),
    }
}

fn iter_len(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("iter.len expects one iterable"));
    };
    iterable_items(gc, value, "iter.len").map(|items| usize_value(items.len()))
}

fn iter_is_empty(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("iter.is_empty expects one iterable"));
    };
    iterable_items(gc, value, "iter.is_empty").map(|items| Value::Bool(items.is_empty()))
}

fn iter_get(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, index] = args else {
        return Err(BuiltinError::new("iter.get expects iterable and index"));
    };
    let index = index_value(index, "iter.get")?;
    match iterable_items(gc, value, "iter.get")?.get(index).cloned() {
        Some(value) => option_some(gc, value),
        None => option_none(gc),
    }
}

fn iter_to_array(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("iter.to_array expects one iterable"));
    };
    array_value(gc, iterable_items(gc, value, "iter.to_array")?)
}

fn iter_for_each(
    gc: &GcHeap,
    args: &[Value],
    callbacks: &mut dyn BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    let [value, callback] = args else {
        return Err(BuiltinError::new(
            "iter.for_each expects iterable and callback",
        ));
    };
    let callback = callback_id(callback, "iter.for_each")?;
    for item in iterable_items(gc, value, "iter.for_each")? {
        let result = callbacks.call(callback, &[item])?;
        if result != Value::Unit {
            return Err(BuiltinError::new("iter.for_each callback must return unit"));
        }
    }
    Ok(Value::Unit)
}

fn math_min(args: &[Value]) -> Result<Value, BuiltinError> {
    let [lhs, rhs] = args else {
        return Err(BuiltinError::new("math.min expects two values"));
    };
    ordered_pair(lhs, rhs, "math.min", |ordering| ordering <= 0)
}

fn math_max(args: &[Value]) -> Result<Value, BuiltinError> {
    let [lhs, rhs] = args else {
        return Err(BuiltinError::new("math.max expects two values"));
    };
    ordered_pair(lhs, rhs, "math.max", |ordering| ordering >= 0)
}

fn math_clamp(args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, min, max] = args else {
        return Err(BuiltinError::new("math.clamp expects value, min, and max"));
    };
    if compare_ordered(min, max, "math.clamp")? > 0 {
        return Err(BuiltinError::new("math.clamp expects min <= max"));
    }
    if compare_ordered(value, min, "math.clamp")? < 0 {
        return Ok(min.clone());
    }
    if compare_ordered(value, max, "math.clamp")? > 0 {
        return Ok(max.clone());
    }
    Ok(value.clone())
}

fn math_abs(args: &[Value]) -> Result<Value, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new("math.abs expects one value"));
    };
    match value {
        Value::I32(value) => value
            .checked_abs()
            .map(Value::I32)
            .ok_or_else(|| BuiltinError::new("math.abs overflow")),
        Value::I64(value) => value
            .checked_abs()
            .map(Value::I64)
            .ok_or_else(|| BuiltinError::new("math.abs overflow")),
        Value::F32(value) if value.is_finite() => Ok(Value::F32(value.abs())),
        Value::F64(value) if value.is_finite() => Ok(Value::F64(value.abs())),
        _ => Err(BuiltinError::new(
            "math.abs expects finite signed numeric value",
        )),
    }
}

fn math_sqrt(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::F64(value)] = args else {
        return Err(BuiltinError::new("math.sqrt expects one f64 value"));
    };
    if !value.is_finite() || *value < 0.0 {
        return Err(BuiltinError::new(
            "math.sqrt expects non-negative finite f64",
        ));
    }
    Ok(Value::F64(value.sqrt()))
}

fn math_unary_f64(
    args: &[Value],
    name: &'static str,
    f: impl FnOnce(f64) -> f64,
) -> Result<Value, BuiltinError> {
    let [Value::F64(value)] = args else {
        return Err(BuiltinError::new(format!("{name} expects one f64 value")));
    };
    if !value.is_finite() {
        return Err(BuiltinError::new(format!(
            "{name} expects finite f64 value"
        )));
    }
    let result = f(*value);
    if !result.is_finite() {
        return Err(BuiltinError::new(format!("{name} produced non-finite f64")));
    }
    Ok(Value::F64(result))
}

fn debug_print(args: &[Value]) -> Result<Value, BuiltinError> {
    let _ = one_string(args, "debug.print")?;
    Ok(Value::Unit)
}

fn debug_assert(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Bool(condition), Value::Str(message)] = args else {
        return Err(BuiltinError::new(
            "debug.assert expects bool and string message",
        ));
    };
    if *condition {
        Ok(Value::Unit)
    } else {
        Err(BuiltinError::new(format!("debug.assert failed: {message}")))
    }
}

fn debug_assert_eq(args: &[Value]) -> Result<Value, BuiltinError> {
    let [lhs, rhs, Value::Str(message)] = args else {
        return Err(BuiltinError::new(
            "debug.assert_eq expects two values and string message",
        ));
    };
    if lhs == rhs {
        Ok(Value::Unit)
    } else {
        Err(BuiltinError::new(format!(
            "debug.assert_eq failed: {message}"
        )))
    }
}

fn debug_panic(args: &[Value]) -> Result<Value, BuiltinError> {
    let message = one_string(args, "debug.panic")?;
    Err(BuiltinError::new(format!("debug.panic: {message}")))
}

fn one_array(args: &[Value], name: &'static str) -> Result<crate::gc::HeapObjectId, BuiltinError> {
    let [Value::Array(handle)] = args else {
        return Err(BuiltinError::new(format!("{name} expects one array")));
    };
    Ok(*handle)
}

fn one_map(args: &[Value], name: &'static str) -> Result<crate::gc::HeapObjectId, BuiltinError> {
    let [Value::Map(handle)] = args else {
        return Err(BuiltinError::new(format!("{name} expects one map")));
    };
    Ok(*handle)
}

fn one_set(args: &[Value], name: &'static str) -> Result<crate::gc::HeapObjectId, BuiltinError> {
    let [Value::Set(handle)] = args else {
        return Err(BuiltinError::new(format!("{name} expects one set")));
    };
    Ok(*handle)
}

fn two_sets(
    args: &[Value],
    name: &'static str,
) -> Result<(crate::gc::HeapObjectId, crate::gc::HeapObjectId), BuiltinError> {
    let [Value::Set(lhs), Value::Set(rhs)] = args else {
        return Err(BuiltinError::new(format!("{name} expects two sets")));
    };
    Ok((*lhs, *rhs))
}

fn one_string<'a>(args: &'a [Value], name: &'static str) -> Result<&'a str, BuiltinError> {
    let [Value::Str(value)] = args else {
        return Err(BuiltinError::new(format!("{name} expects one string")));
    };
    Ok(value)
}

fn index_value(value: &Value, name: &'static str) -> Result<usize, BuiltinError> {
    match value {
        Value::I32(index) if *index >= 0 => Ok(*index as usize),
        Value::I64(index) if *index >= 0 => Ok(*index as usize),
        _ => Err(BuiltinError::new(format!(
            "{name} expects non-negative integer index"
        ))),
    }
}

fn usize_value(value: usize) -> Value {
    Value::I64(value as i64)
}

fn require_hash_key(value: &Value, name: &'static str) -> Result<(), BuiltinError> {
    MapKey::from_value(value)
        .map(|_| ())
        .ok_or_else(|| BuiltinError::new(format!("{name} expects bool, integer, or string key")))
}

fn array_value(gc: &GcHeap, values: Vec<Value>) -> Result<Value, BuiltinError> {
    gc.alloc_array(values)
        .map(Value::Array)
        .ok_or_else(|| BuiltinError::new("could not allocate standard array value"))
}

fn set_value(gc: &GcHeap, values: Vec<Value>) -> Result<Value, BuiltinError> {
    gc.alloc_set(values)
        .map(Value::Set)
        .ok_or_else(|| BuiltinError::new("could not allocate standard set value"))
}

fn option_some(gc: &GcHeap, value: Value) -> Result<Value, BuiltinError> {
    enum_value(gc, "Option", "Some", vec![value])
}

fn option_none(gc: &GcHeap) -> Result<Value, BuiltinError> {
    enum_value(gc, "Option", "None", Vec::new())
}

fn result_ok(gc: &GcHeap, value: Value) -> Result<Value, BuiltinError> {
    enum_value(gc, "Result", "Ok", vec![value])
}

fn result_err(gc: &GcHeap, value: Value) -> Result<Value, BuiltinError> {
    enum_value(gc, "Result", "Err", vec![value])
}

fn enum_value(
    gc: &GcHeap,
    name: &'static str,
    variant: &'static str,
    fields: Vec<Value>,
) -> Result<Value, BuiltinError> {
    gc.alloc_enum(name.to_owned(), variant.to_owned(), fields)
        .map(Value::Enum)
        .ok_or_else(|| BuiltinError::new("could not allocate standard enum value"))
}

fn option_value(
    gc: &GcHeap,
    args: &[Value],
    name: &'static str,
) -> Result<crate::value::EnumValueSnapshot, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new(format!("{name} expects one option")));
    };
    option_snapshot(gc, value, name)
}

fn result_value(
    gc: &GcHeap,
    args: &[Value],
    name: &'static str,
) -> Result<crate::value::EnumValueSnapshot, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new(format!("{name} expects one result")));
    };
    result_snapshot(gc, value, name)
}

fn option_snapshot(
    gc: &GcHeap,
    value: &Value,
    name: &'static str,
) -> Result<crate::value::EnumValueSnapshot, BuiltinError> {
    let Value::Enum(handle) = value else {
        return Err(BuiltinError::new(format!("{name} expects Option value")));
    };
    let snapshot = gc
        .enum_snapshot(*handle)
        .ok_or_else(|| BuiltinError::new(format!("{name} expects valid enum handle")))?;
    match (
        snapshot.name.as_str(),
        snapshot.variant.as_str(),
        snapshot.fields.len(),
    ) {
        ("Option", "Some", 1) | ("Option", "None", 0) => Ok(snapshot),
        _ => Err(BuiltinError::new(format!("{name} expects Option value"))),
    }
}

fn result_snapshot(
    gc: &GcHeap,
    value: &Value,
    name: &'static str,
) -> Result<crate::value::EnumValueSnapshot, BuiltinError> {
    let Value::Enum(handle) = value else {
        return Err(BuiltinError::new(format!("{name} expects Result value")));
    };
    let snapshot = gc
        .enum_snapshot(*handle)
        .ok_or_else(|| BuiltinError::new(format!("{name} expects valid enum handle")))?;
    match (
        snapshot.name.as_str(),
        snapshot.variant.as_str(),
        snapshot.fields.len(),
    ) {
        ("Result", "Ok", 1) | ("Result", "Err", 1) => Ok(snapshot),
        _ => Err(BuiltinError::new(format!("{name} expects Result value"))),
    }
}

fn option_payload(gc: &GcHeap, value: &Value, name: &'static str) -> Result<Value, BuiltinError> {
    option_snapshot(gc, value, name)?
        .fields
        .into_iter()
        .next()
        .ok_or_else(|| BuiltinError::new(format!("{name} option has no payload")))
}

fn result_payload(gc: &GcHeap, value: &Value, name: &'static str) -> Result<Value, BuiltinError> {
    result_snapshot(gc, value, name)?
        .fields
        .into_iter()
        .next()
        .ok_or_else(|| BuiltinError::new(format!("{name} result has no payload")))
}

fn callback_id(value: &Value, name: &'static str) -> Result<EphemeralValueId, BuiltinError> {
    match value {
        Value::Ephemeral(EphemeralValue::Runtime(id)) => Ok(*id),
        _ => Err(BuiltinError::new(format!(
            "{name} expects runtime callback token"
        ))),
    }
}

fn iterable_items(
    gc: &GcHeap,
    value: &Value,
    name: &'static str,
) -> Result<Vec<Value>, BuiltinError> {
    match value {
        Value::Array(handle) => gc
            .array_snapshot(*handle)
            .ok_or_else(|| BuiltinError::new(format!("{name} expects valid array handle"))),
        Value::Map(handle) => gc
            .map_snapshot(*handle)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|(key, value)| Value::Tuple(vec![key, value]))
                    .collect()
            })
            .ok_or_else(|| BuiltinError::new(format!("{name} expects valid map handle"))),
        Value::Set(handle) => gc
            .set_snapshot(*handle)
            .ok_or_else(|| BuiltinError::new(format!("{name} expects valid set handle"))),
        Value::Str(value) => Ok(value
            .chars()
            .map(|character| Value::Str(character.to_string()))
            .collect()),
        _ => Err(BuiltinError::new(format!(
            "{name} expects array, map, set, or string"
        ))),
    }
}

fn ordered_pair(
    lhs: &Value,
    rhs: &Value,
    name: &'static str,
    keep_lhs: impl FnOnce(i8) -> bool,
) -> Result<Value, BuiltinError> {
    let ordering = compare_ordered(lhs, rhs, name)?;
    if keep_lhs(ordering) {
        Ok(lhs.clone())
    } else {
        Ok(rhs.clone())
    }
}

fn compare_ordered(lhs: &Value, rhs: &Value, name: &'static str) -> Result<i8, BuiltinError> {
    match (lhs, rhs) {
        (Value::I32(lhs), Value::I32(rhs)) => Ok(ordering_value(lhs.cmp(rhs))),
        (Value::I64(lhs), Value::I64(rhs)) => Ok(ordering_value(lhs.cmp(rhs))),
        (Value::F32(lhs), Value::F32(rhs)) if lhs.is_finite() && rhs.is_finite() => {
            compare_f64(*lhs as f64, *rhs as f64)
        }
        (Value::F64(lhs), Value::F64(rhs)) if lhs.is_finite() && rhs.is_finite() => {
            compare_f64(*lhs, *rhs)
        }
        _ => Err(BuiltinError::new(format!(
            "{name} expects same-type finite ordered numbers"
        ))),
    }
}

fn ordering_value(ordering: std::cmp::Ordering) -> i8 {
    match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn compare_f64(lhs: f64, rhs: f64) -> Result<i8, BuiltinError> {
    lhs.partial_cmp(&rhs)
        .map(ordering_value)
        .ok_or_else(|| BuiltinError::new("float comparison is unordered"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_ir::builtin::surface::StandardIntrinsic;

    use crate::{
        gc::{GcHeap, GcHeapConfig},
        value::EphemeralValue,
    };

    #[derive(Default)]
    struct TestCallbacks {
        seen: Vec<Value>,
        result_value: Option<Value>,
    }

    impl BuiltinCallbacks for TestCallbacks {
        fn call(&mut self, id: EphemeralValueId, args: &[Value]) -> Result<Value, BuiltinError> {
            match id.0 {
                1 => {
                    let [Value::I32(value)] = args else {
                        return Err(BuiltinError::new("expected i32 callback input"));
                    };
                    Ok(Value::I32(value + 1))
                }
                2 => {
                    let [value] = args else {
                        return Err(BuiltinError::new("expected callback input"));
                    };
                    self.seen.push(value.clone());
                    Ok(Value::Unit)
                }
                3 => self
                    .result_value
                    .clone()
                    .ok_or_else(|| BuiltinError::new("missing result callback value")),
                _ => Err(BuiltinError::new("unknown callback")),
            }
        }
    }

    fn call(
        gc: &GcHeap,
        intrinsic: StandardIntrinsic,
        args: &[Value],
    ) -> Result<Value, BuiltinError> {
        invoke(gc, intrinsic, args)
    }

    fn callback(id: u64) -> Value {
        Value::Ephemeral(EphemeralValue::Runtime(EphemeralValueId(id)))
    }

    fn option_variant(gc: &GcHeap, value: &Value) -> (String, Vec<Value>) {
        let Value::Enum(handle) = value else {
            panic!("expected enum value");
        };
        let snapshot = gc.enum_snapshot(*handle).unwrap();
        assert_eq!(snapshot.name, "Option");
        (snapshot.variant, snapshot.fields)
    }

    fn result_variant(gc: &GcHeap, value: &Value) -> (String, Vec<Value>) {
        let Value::Enum(handle) = value else {
            panic!("expected enum value");
        };
        let snapshot = gc.enum_snapshot(*handle).unwrap();
        assert_eq!(snapshot.name, "Result");
        (snapshot.variant, snapshot.fields)
    }

    #[test]
    fn builtin_standard_array_helpers_mutate_and_return_options() {
        let gc = GcHeap::new(GcHeapConfig::default());
        let array = Value::Array(gc.alloc_array(vec![Value::I32(1)]).unwrap());

        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::ArrayLen,
                std::slice::from_ref(&array)
            )
            .unwrap(),
            Value::I64(1)
        );
        call(
            &gc,
            StandardIntrinsic::ArrayPush,
            &[array.clone(), Value::I32(3)],
        )
        .unwrap();
        call(
            &gc,
            StandardIntrinsic::ArrayInsert,
            &[array.clone(), Value::I64(1), Value::I32(2)],
        )
        .unwrap();
        assert_eq!(
            gc.array_snapshot(match array {
                Value::Array(handle) => handle,
                _ => unreachable!(),
            })
            .unwrap(),
            vec![Value::I32(1), Value::I32(2), Value::I32(3)]
        );

        let removed = call(
            &gc,
            StandardIntrinsic::ArrayRemove,
            &[array.clone(), Value::I32(1)],
        )
        .unwrap();
        assert_eq!(
            option_variant(&gc, &removed),
            ("Some".to_owned(), vec![Value::I32(2)])
        );
        let missing = call(&gc, StandardIntrinsic::ArrayGet, &[array, Value::I32(99)]).unwrap();
        assert_eq!(
            option_variant(&gc, &missing),
            ("None".to_owned(), Vec::new())
        );
    }

    #[test]
    fn builtin_standard_map_helpers_preserve_order_and_return_options() {
        let gc = GcHeap::new(GcHeapConfig::default());
        let map = call(&gc, StandardIntrinsic::MapNew, &[]).unwrap();
        call(
            &gc,
            StandardIntrinsic::MapInsert,
            &[map.clone(), Value::Str("hp".to_owned()), Value::I32(100)],
        )
        .unwrap();
        call(
            &gc,
            StandardIntrinsic::MapInsert,
            &[map.clone(), Value::Str("mp".to_owned()), Value::I32(40)],
        )
        .unwrap();

        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::MapContainsKey,
                &[map.clone(), Value::Str("hp".to_owned())]
            )
            .unwrap(),
            Value::Bool(true)
        );
        let keys = call(&gc, StandardIntrinsic::MapKeys, std::slice::from_ref(&map)).unwrap();
        let Value::Array(keys) = keys else {
            panic!("expected key array");
        };
        assert_eq!(
            gc.array_snapshot(keys).unwrap(),
            vec![Value::Str("hp".to_owned()), Value::Str("mp".to_owned())]
        );
        let removed = call(
            &gc,
            StandardIntrinsic::MapRemove,
            &[map.clone(), Value::Str("hp".to_owned())],
        )
        .unwrap();
        assert_eq!(
            option_variant(&gc, &removed),
            ("Some".to_owned(), vec![Value::I32(100)])
        );
        let missing = call(
            &gc,
            StandardIntrinsic::MapGet,
            &[map, Value::Str("hp".to_owned())],
        )
        .unwrap();
        assert_eq!(
            option_variant(&gc, &missing),
            ("None".to_owned(), Vec::new())
        );
    }

    #[test]
    fn builtin_standard_set_helpers_use_ordered_algebra() {
        let gc = GcHeap::new(GcHeapConfig::default());
        let lhs = Value::Set(gc.alloc_set(vec![Value::I32(1), Value::I32(2)]).unwrap());
        let rhs = Value::Set(gc.alloc_set(vec![Value::I32(2), Value::I32(3)]).unwrap());

        let union = call(
            &gc,
            StandardIntrinsic::SetUnion,
            &[lhs.clone(), rhs.clone()],
        )
        .unwrap();
        let intersection = call(
            &gc,
            StandardIntrinsic::SetIntersection,
            &[lhs.clone(), rhs.clone()],
        )
        .unwrap();
        let difference = call(&gc, StandardIntrinsic::SetDifference, &[lhs, rhs]).unwrap();

        let Value::Set(union) = union else {
            panic!("expected union set");
        };
        let Value::Set(intersection) = intersection else {
            panic!("expected intersection set");
        };
        let Value::Set(difference) = difference else {
            panic!("expected difference set");
        };
        assert_eq!(
            gc.set_snapshot(union).unwrap(),
            vec![Value::I32(1), Value::I32(2), Value::I32(3)]
        );
        assert_eq!(gc.set_snapshot(intersection).unwrap(), vec![Value::I32(2)]);
        assert_eq!(gc.set_snapshot(difference).unwrap(), vec![Value::I32(1)]);
    }

    #[test]
    fn builtin_standard_string_helpers_validate_utf8_boundaries() {
        let gc = GcHeap::new(GcHeapConfig::default());
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::StringLenBytes,
                &[Value::Str("éx".to_owned())]
            )
            .unwrap(),
            Value::I64(3)
        );
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::StringLenChars,
                &[Value::Str("éx".to_owned())]
            )
            .unwrap(),
            Value::I64(2)
        );
        let good = call(
            &gc,
            StandardIntrinsic::StringSlice,
            &[Value::Str("éx".to_owned()), Value::I64(0), Value::I64(2)],
        )
        .unwrap();
        assert_eq!(
            option_variant(&gc, &good),
            ("Some".to_owned(), vec![Value::Str("é".to_owned())])
        );
        let bad = call(
            &gc,
            StandardIntrinsic::StringSlice,
            &[Value::Str("éx".to_owned()), Value::I64(1), Value::I64(2)],
        )
        .unwrap();
        assert_eq!(option_variant(&gc, &bad), ("None".to_owned(), Vec::new()));
    }

    #[test]
    fn builtin_standard_option_result_helpers_use_standard_enum_values() {
        let gc = GcHeap::new(GcHeapConfig::default());
        let some = option_some(&gc, Value::I32(10)).unwrap();
        let none = option_none(&gc).unwrap();
        let ok = result_ok(&gc, Value::I32(7)).unwrap();
        let err = result_err(&gc, Value::Str("no".to_owned())).unwrap();

        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::OptionUnwrapOr,
                &[some.clone(), Value::I32(0)]
            )
            .unwrap(),
            Value::I32(10)
        );
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::OptionUnwrapOr,
                &[none, Value::I32(0)]
            )
            .unwrap(),
            Value::I32(0)
        );
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::ResultUnwrapOr,
                &[ok.clone(), Value::I32(0)]
            )
            .unwrap(),
            Value::I32(7)
        );
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::ResultIsErr,
                std::slice::from_ref(&err)
            )
            .unwrap(),
            Value::Bool(true)
        );

        let mut callbacks = TestCallbacks {
            result_value: Some(result_ok(&gc, Value::I32(99)).unwrap()),
            ..TestCallbacks::default()
        };
        let mapped = invoke_with_callbacks(
            &gc,
            StandardIntrinsic::OptionMap,
            &[some, callback(1)],
            &mut callbacks,
        )
        .unwrap();
        assert_eq!(
            option_variant(&gc, &mapped),
            ("Some".to_owned(), vec![Value::I32(11)])
        );

        let chained = invoke_with_callbacks(
            &gc,
            StandardIntrinsic::ResultAndThen,
            &[ok, callback(3)],
            &mut callbacks,
        )
        .unwrap();
        assert_eq!(
            result_variant(&gc, &chained),
            ("Ok".to_owned(), vec![Value::I32(99)])
        );
    }

    #[test]
    fn builtin_standard_iter_helpers_cover_arrays_maps_sets_strings_and_callbacks() {
        let gc = GcHeap::new(GcHeapConfig::default());
        let array = Value::Array(gc.alloc_array(vec![Value::I32(1), Value::I32(2)]).unwrap());
        let map = Value::Map(
            gc.alloc_map(vec![
                (Value::Str("a".to_owned()), Value::I32(1)),
                (Value::Str("b".to_owned()), Value::I32(2)),
            ])
            .unwrap(),
        );
        let set = Value::Set(gc.alloc_set(vec![Value::I32(3), Value::I32(4)]).unwrap());

        assert_eq!(
            call(&gc, StandardIntrinsic::IterLen, std::slice::from_ref(&map)).unwrap(),
            Value::I64(2)
        );
        let first_map_entry = call(&gc, StandardIntrinsic::IterGet, &[map, Value::I32(0)]).unwrap();
        assert_eq!(
            option_variant(&gc, &first_map_entry),
            (
                "Some".to_owned(),
                vec![Value::Tuple(vec![
                    Value::Str("a".to_owned()),
                    Value::I32(1)
                ])]
            )
        );
        let set_array = call(
            &gc,
            StandardIntrinsic::IterToArray,
            std::slice::from_ref(&set),
        )
        .unwrap();
        let Value::Array(set_array) = set_array else {
            panic!("expected set item array");
        };
        assert_eq!(
            gc.array_snapshot(set_array).unwrap(),
            vec![Value::I32(3), Value::I32(4)]
        );
        let first_char = call(
            &gc,
            StandardIntrinsic::IterGet,
            &[Value::Str("ab".to_owned()), Value::I32(0)],
        )
        .unwrap();
        assert_eq!(
            option_variant(&gc, &first_char),
            ("Some".to_owned(), vec![Value::Str("a".to_owned())])
        );

        let mut callbacks = TestCallbacks::default();
        invoke_with_callbacks(
            &gc,
            StandardIntrinsic::IterForEach,
            &[array, callback(2)],
            &mut callbacks,
        )
        .unwrap();
        assert_eq!(callbacks.seen, vec![Value::I32(1), Value::I32(2)]);
    }

    #[test]
    fn builtin_standard_math_and_debug_helpers_are_deterministic() {
        let gc = GcHeap::new(GcHeapConfig::default());
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::MathMin,
                &[Value::I64(8), Value::I64(3)]
            )
            .unwrap(),
            Value::I64(3)
        );
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::MathClamp,
                &[Value::I32(12), Value::I32(0), Value::I32(10)]
            )
            .unwrap(),
            Value::I32(10)
        );
        assert_eq!(
            call(&gc, StandardIntrinsic::MathSqrt, &[Value::F64(9.0)]).unwrap(),
            Value::F64(3.0)
        );
        assert!(call(&gc, StandardIntrinsic::MathSqrt, &[Value::F64(-1.0)]).is_err());
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::DebugAssertEq,
                &[Value::I32(1), Value::I32(1), Value::Str("same".to_owned())]
            )
            .unwrap(),
            Value::Unit
        );
        assert!(
            call(
                &gc,
                StandardIntrinsic::DebugPanic,
                &[Value::Str("boom".to_owned())]
            )
            .is_err()
        );
    }
}
