use crate::value::EnumTag;
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
        MapContainsKey | MapGet | MapInsert | MapRemove | SetContains | SetInsert | SetRemove => {
            if let Some(collection) = args.first() {
                gc.ensure_key_mode(collection, false)?;
            }
        }

        _ => {}
    }
    match intrinsic {
        StringParse => Err(BuiltinError::new("parse requires static dispatch")),
        ParseNumber(ty) => crate::parsing::parse(gc, ty, args, false).map_err(Into::into),
        ParseRadix(ty) => crate::parsing::parse(gc, ty, args, true).map_err(Into::into),
        Integer(operation, ty) => {
            crate::numeric::integer_method(gc, operation, ty, args).map_err(Into::into)
        }
        KeyLookupBegin => Err(BuiltinError::new("key lookup requires an execution frame")),
        KeyCandidates => {
            let [collection, Value::I64(hash)] = args else {
                return Err(BuiltinError::new("invalid key candidates arguments"));
            };
            array_value(gc, gc.custom_candidates(collection, *hash)?)
        }
        KeyMapGet | KeyMapInsert | KeyMapRemove | KeySetContains | KeySetInsert | KeySetRemove => {
            custom_key_operation(gc, intrinsic, args)
        }
        ValuePartialCmp | ValueCmp => {
            let [a, b] = args else {
                return Err(BuiltinError::new("comparison requires two operands"));
            };
            let ordering = crate::value_semantics::builtin_order(gc, a, b)?;
            let Some(ordering) = ordering else {
                if intrinsic == ValueCmp {
                    return Err(BuiltinError::new("total comparison cannot be unordered"));
                }
                return option_none(gc);
            };
            let tag = match ordering {
                std::cmp::Ordering::Less => EnumTag::OrderingLess,
                std::cmp::Ordering::Equal => EnumTag::OrderingEqual,
                std::cmp::Ordering::Greater => EnumTag::OrderingGreater,
            };
            let value = Value::Enum(gc.alloc_enum(tag, vec![])?);
            if intrinsic == ValuePartialCmp {
                option_some(gc, value)
            } else {
                Ok(value)
            }
        }
        ValueEq => {
            let [a, b] = args else {
                return Err(BuiltinError::new("eq expects two operands"));
            };
            crate::value_semantics::script_equal(gc, a, b)
                .map(Value::Bool)
                .map_err(BuiltinError::from)
        }
        ValueHash => {
            let [value] = args else {
                return Err(BuiltinError::new("hash expects one operand"));
            };
            MapKey::from_value(gc, value)
                .map(|key| Value::I64(key.script_hash()))
                .ok_or_else(|| {
                    BuiltinError::new("value has no hash semantics or exceeds key size limit")
                })
        }
        ValueDebug | ValueDisplay => {
            let [value] = args else {
                return Err(BuiltinError::new("format expects one operand"));
            };
            crate::value_semantics::format_value(gc, value, intrinsic == ValueDebug)
                .map(Value::Str)
                .map_err(BuiltinError::from)
        }
        ArrayListNew => {
            if !args.is_empty() {
                return Err(BuiltinError::new("new expects no arguments"));
            }
            Ok(Value::Array(gc.alloc_array(vec![])?))
        }
        MapKeys | MapValues | MapEntries | ArrayCopyFrom | ArrayListFromFn | ArrayListFrom
        | LinkedHashMapFrom | LinkedHashSetFrom => Err(BuiltinError::new(
            "collection factories must be lowered to checked construction",
        )),
        ArrayLen => array_len(gc, args),
        ArrayIsEmpty => array_is_empty(gc, args),
        ArrayGet => array_get(gc, args),
        ArrayPush => array_push(gc, args),
        ArrayPop => array_pop(gc, args),
        ArrayInsert => array_insert(gc, args),
        ArrayRemove => array_remove(gc, args),
        ArrayWithCapacity | ArrayCapacity | ArrayReserve | MapWithCapacity | MapCapacity
        | MapReserve | SetWithCapacity | SetCapacity | SetReserve => {
            collection_capacity(gc, intrinsic, args)
        }
        ArrayExtend => Err(BuiltinError::new(
            "extend requires prepared source lowering",
        )),
        ArraySwap | ArrayReverse | ArrayTruncate | ArrayExtendStorage | ArraySwapRemove => {
            array_mutation(gc, intrinsic, args)
        }
        ArrayJoin => array_join(gc, args),
        ArrayClear => array_clear(gc, args),
        ArrayCopyWithin => Err(BuiltinError::new("range bounds require static lowering")),
        ArrayCopyWithinBounds => {
            let [Value::Array(target), start, end, destination] = args else {
                return Err(BuiltinError::new("invalid copy_within operands"));
            };
            let destination = usize::try_from(match destination {
                Value::U64(n) => *n,
                _ => return Err(BuiltinError::new("invalid copy destination")),
            })
            .map_err(|_| BuiltinError::new("copy destination exceeds platform capacity"))?;
            let start = crate::range::index_bound(gc, start)?;
            let end = crate::range::index_bound(gc, end)?;
            gc.array_copy_within(*target, start, end, destination)?;
            Ok(Value::Unit)
        }
        ArrayFill => {
            let [Value::Array(target), value] = args else {
                return Err(BuiltinError::new("array.fill expects an array and value"));
            };
            gc.array_fill(*target, value.clone())?;
            Ok(Value::Unit)
        }
        ArrayCopyFromStorage => {
            let [Value::Array(target), Value::Array(source)] = args else {
                return Err(BuiltinError::new("array.copy_from expects two arrays"));
            };
            gc.array_copy_from(*target, *source)?;
            Ok(Value::Unit)
        }
        LinkedHashMapNew => map_new(gc, args),
        MapLen => map_len(gc, args),
        MapIsEmpty => map_is_empty(gc, args),
        MapContainsKey => map_contains_key(gc, args),
        MapGet => map_get(gc, args),
        MapInsert => map_insert(gc, args),
        MapRemove => map_remove(gc, args),
        MapClear => map_clear(gc, args),
        MapKeysStorage => map_keys(gc, args),
        MapValuesStorage => map_values(gc, args),
        MapEntriesStorage => map_entries(gc, args),
        LinkedHashSetNew => set_new(gc, args),
        SetLen => set_len(gc, args),
        SetIsEmpty => set_is_empty(gc, args),
        SetContains => set_contains(gc, args),
        SetInsert => set_insert(gc, args),
        SetRemove => set_remove(gc, args),
        SetClear => set_clear(gc, args),
        SetToArray => set_to_array(gc, args),
        StringLenBytes => string_len_bytes(args),
        StringLenChars => string_len_chars(args),
        StringIsEmpty => string_is_empty(args),
        StringConcat => string_concat(args),
        StringContains => string_contains(args),
        StringStartsWith => string_starts_with(args),
        StringEndsWith => string_ends_with(args),
        StringSlice => string_slice(gc, args),
        StringReplace
        | StringReplaceN
        | StringRepeat
        | StringIsAscii
        | StringEqIgnoreAsciiCase
        | StringToAsciiLowercase
        | StringToAsciiUppercase
        | StringToLowercase
        | StringToUppercase
        | StringIsCharBoundary => string_transform(intrinsic, args),
        StringSplit
        | StringSplitN
        | StringSplitWhitespace
        | StringLines
        | StringBytes
        | StringCharIndices => Err(BuiltinError::new(
            "string traversal requires iterator lowering",
        )),
        StringSplitOnce | StringRsplitOnce => string_split_once(gc, intrinsic, args),
        StringTrim | StringTrimStart | StringTrimEnd | StringFind | StringRfind
        | StringStripPrefix | StringStripSuffix => string_query(gc, intrinsic, args),
        OptionUnwrapOrElse | OptionOrElse | OptionMapOr | OptionMapOrElse | OptionFilter
        | OptionIsSomeAnd | OptionZip | OptionFlatten | OptionTranspose | ResultUnwrapOrElse
        | ResultOrElse | ResultMapOr | ResultMapOrElse | ResultOk | ResultErr | ResultIsOkAnd
        | ResultIsErrAnd | ResultFlatten | ResultTranspose => {
            Err(BuiltinError::new("enum combinators require frame lowering"))
        }
        OptionIsSome => option_is_some(gc, args),
        OptionIsNone => option_is_none(gc, args),
        OptionUnwrapOr => option_unwrap_or(gc, args),
        OptionMap => option_map(gc, args, callbacks),
        OptionAndThen => option_and_then(gc, args, callbacks),
        OptionOkOr | OptionOkOrElse => {
            option_ok_or(gc, args, callbacks, intrinsic == OptionOkOrElse)
        }
        ResultIsOk => result_is_ok(gc, args),
        ResultIsErr => result_is_err(gc, args),
        ResultUnwrapOr => result_unwrap_or(gc, args),
        ResultMap => result_map(gc, args, callbacks),
        ResultMapErr => result_map_err(gc, args, callbacks),
        ResultAndThen => result_and_then(gc, args, callbacks),
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
        DebugAssertEq => debug_assert_eq(gc, args),
        DebugPanic => debug_panic(args),
    }
}

fn collection_capacity(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    use StandardIntrinsic::*;
    if matches!(
        intrinsic,
        ArrayWithCapacity | MapWithCapacity | SetWithCapacity
    ) {
        let [Value::U64(capacity)] = args else {
            return Err(BuiltinError::new("capacity constructor requires usize"));
        };
        let capacity = usize::try_from(*capacity)
            .map_err(|_| BuiltinError::new("capacity exceeds platform limit"))?;
        let value = match intrinsic {
            ArrayWithCapacity => Value::Array(gc.alloc_array(vec![])?),
            MapWithCapacity => Value::Map(gc.alloc_map(vec![])?),
            _ => Value::Set(gc.alloc_set(vec![])?),
        };
        gc.reserve_collection(&value, capacity)?;
        return Ok(value);
    }
    match args {
        [value] if matches!(intrinsic, ArrayCapacity | MapCapacity | SetCapacity) => {
            Ok(Value::U64(gc.collection_capacity(value)? as u64))
        }
        [value, Value::U64(additional)]
            if matches!(intrinsic, ArrayReserve | MapReserve | SetReserve) =>
        {
            let additional = usize::try_from(*additional)
                .map_err(|_| BuiltinError::new("capacity exceeds platform limit"))?;
            gc.reserve_collection(value, additional)?;
            Ok(Value::Unit)
        }
        _ => Err(BuiltinError::new("invalid capacity operands")),
    }
}

fn array_mutation(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    use StandardIntrinsic::*;
    let Some(Value::Array(id)) = args.first() else {
        return Err(BuiltinError::new("array mutation requires an array"));
    };
    let index = |value: &Value| match value {
        Value::U64(n) => {
            usize::try_from(*n).map_err(|_| BuiltinError::new("array index exceeds capacity"))
        }
        _ => Err(BuiltinError::new("invalid array index")),
    };
    match (intrinsic, args) {
        (ArraySwap, [_, a, b]) => gc.array_swap(*id, index(a)?, index(b)?)?,
        (ArrayReverse, [_]) => gc.array_reverse(*id)?,
        (ArrayTruncate, [_, len]) => gc.array_truncate(*id, index(len)?)?,
        (ArrayExtendStorage, [_, Value::Array(source)]) => gc.array_extend(*id, *source)?,
        (ArraySwapRemove, [_, position]) => {
            let position = index(position)?;
            // Prepare the return value before committing removal.
            let result = match gc.array_get(*id, position) {
                Some(value) => option_some(gc, value)?,
                None => option_none(gc)?,
            };
            gc.array_swap_remove(*id, position)?;
            return Ok(result);
        }
        _ => return Err(BuiltinError::new("invalid array mutation operands")),
    }
    Ok(Value::Unit)
}

fn array_join(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Array(handle), Value::Str(separator)] = args else {
        return Err(BuiltinError::new(
            "array.join expects a string array and separator",
        ));
    };
    gc.with_array(*handle, |values| {
        let overflow = || {
            BuiltinError::from(crate::error::RuntimeError::resource_limit(
                "joined string size",
            ))
        };
        let mut length = separator
            .len()
            .checked_mul(values.len().saturating_sub(1))
            .ok_or_else(overflow)?;
        for value in values {
            let Value::Str(value) = value else {
                return Err(BuiltinError::new("array.join expects string elements"));
            };
            length = length.checked_add(value.len()).ok_or_else(overflow)?;
        }
        let mut output = String::new();
        output.try_reserve_exact(length).map_err(|_| overflow())?;
        for (index, value) in values.iter().enumerate() {
            if index != 0 {
                output.push_str(separator);
            }
            let Value::Str(value) = value else {
                unreachable!("validated string element");
            };
            output.push_str(value);
        }
        Ok(Value::Str(output))
    })
    .ok_or_else(|| BuiltinError::new("array.join expects a valid array handle"))?
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
        .map_err(BuiltinError::from)
}

fn array_pop(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_array(args, "array.pop")?;
    gc.ensure_structure_mutable(handle)
        .map_err(BuiltinError::from)?;
    let len = gc
        .array_len(handle)
        .ok_or_else(|| BuiltinError::new("array.pop expects valid array"))?;
    let Some(index) = len.checked_sub(1) else {
        return option_none(gc);
    };
    let value = gc
        .array_get(handle, index)
        .expect("validated final array index");
    let result = option_some(gc, value)?;
    gc.array_pop(handle)
        .map_err(BuiltinError::from)?
        .expect("prepared array pop must contain a value");
    Ok(result)
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
        .map_err(BuiltinError::from)
}

fn array_remove(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Array(handle), index] = args else {
        return Err(BuiltinError::new("array.remove expects array and index"));
    };
    let index = index_value(index, "array.remove")?;
    gc.ensure_structure_mutable(*handle)
        .map_err(BuiltinError::from)?;
    gc.array_len(*handle)
        .ok_or_else(|| BuiltinError::new("array.remove expects valid array"))?;
    let Some(value) = gc.array_get(*handle, index) else {
        return option_none(gc);
    };
    let result = option_some(gc, value)?;
    gc.array_remove(*handle, index)
        .map_err(BuiltinError::from)?
        .expect("prepared array removal must contain a value");
    Ok(result)
}

fn array_clear(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_array(args, "array.clear")?;
    gc.ensure_structure_mutable(handle)
        .map_err(BuiltinError::from)?;
    gc.array_clear(handle)
        .map(|_| Value::Array(handle))
        .map_err(BuiltinError::from)
}

fn map_new(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    if !args.is_empty() {
        return Err(BuiltinError::new("map.new expects no arguments"));
    }
    gc.alloc_map(Vec::new())
        .map(Value::Map)
        .map_err(BuiltinError::from)
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
    require_hash_key(gc, key, "map.contains_key")?;
    gc.map_len(*handle)
        .ok_or_else(|| BuiltinError::new("map.contains_key expects valid map handle"))?;
    Ok(Value::Bool(gc.map_get(*handle, key).is_some()))
}

fn map_get(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Map(handle), key] = args else {
        return Err(BuiltinError::new("map.get expects map and key"));
    };
    require_hash_key(gc, key, "map.get")?;
    match gc.map_get(*handle, key) {
        Some(value) => option_some(gc, value),
        None => option_none(gc),
    }
}

fn map_insert(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Map(handle), key, item] = args else {
        return Err(BuiltinError::new("map.insert expects map, key, and item"));
    };
    require_hash_key(gc, key, "map.insert")?;
    gc.map_insert(*handle, key.clone(), item.clone())
        .map(|_| Value::Map(*handle))
        .map_err(BuiltinError::from)
}

fn map_remove(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Map(handle), key] = args else {
        return Err(BuiltinError::new("map.remove expects map and key"));
    };
    require_hash_key(gc, key, "map.remove")?;
    gc.ensure_structure_mutable(*handle)
        .map_err(BuiltinError::from)?;
    gc.map_len(*handle)
        .ok_or_else(|| BuiltinError::new("map.remove expects valid map"))?;
    let Some(value) = gc.map_get(*handle, key) else {
        return option_none(gc);
    };
    let result = option_some(gc, value)?;
    gc.map_remove(*handle, key)
        .map_err(BuiltinError::from)?
        .expect("prepared map removal must contain a value");
    Ok(result)
}

fn map_clear(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_map(args, "map.clear")?;
    gc.ensure_structure_mutable(handle)
        .map_err(BuiltinError::from)?;
    gc.map_clear(handle)
        .map(|_| Value::Map(handle))
        .map_err(BuiltinError::from)
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
        .map_err(BuiltinError::from)
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
    require_hash_key(gc, item, "set.contains")?;
    gc.set_contains(*handle, item)
        .map(Value::Bool)
        .ok_or_else(|| BuiltinError::new("set.contains expects valid set handle"))
}

fn set_insert(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Set(handle), item] = args else {
        return Err(BuiltinError::new("set.insert expects set and item"));
    };
    require_hash_key(gc, item, "set.insert")?;
    gc.set_insert(*handle, item.clone())
        .map(|_| Value::Set(*handle))
        .map_err(BuiltinError::from)
}

fn set_remove(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Set(handle), item] = args else {
        return Err(BuiltinError::new("set.remove expects set and item"));
    };
    require_hash_key(gc, item, "set.remove")?;
    gc.ensure_structure_mutable(*handle)
        .map_err(BuiltinError::from)?;
    gc.set_remove(*handle, item)
        .map(Value::Bool)
        .map_err(BuiltinError::from)
}

fn set_clear(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_set(args, "set.clear")?;
    gc.ensure_structure_mutable(handle)
        .map_err(BuiltinError::from)?;
    gc.set_clear(handle)
        .map(|_| Value::Set(handle))
        .map_err(BuiltinError::from)
}

fn set_to_array(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let handle = one_set(args, "set.to_array")?;
    let values = gc
        .set_snapshot(handle)
        .ok_or_else(|| BuiltinError::new("set.to_array expects valid set handle"))?;
    array_value(gc, values)
}

fn string_transform(intrinsic: StandardIntrinsic, args: &[Value]) -> Result<Value, BuiltinError> {
    use StandardIntrinsic::*;
    let Some(Value::Str(text)) = args.first() else {
        return Err(BuiltinError::new(
            "string operation requires a string receiver",
        ));
    };
    let allocation = || {
        BuiltinError::from(crate::error::RuntimeError::resource_limit(
            "string result size",
        ))
    };
    match (intrinsic, args) {
        (StringIsAscii, [_]) => Ok(Value::Bool(text.is_ascii())),
        (StringEqIgnoreAsciiCase, [_, Value::Str(other)]) => {
            Ok(Value::Bool(text.eq_ignore_ascii_case(other)))
        }
        (StringIsCharBoundary, [_, Value::U64(index)]) => Ok(Value::Bool(
            usize::try_from(*index).is_ok_and(|index| text.is_char_boundary(index)),
        )),
        (StringToLowercase, [_]) => Ok(Value::Str(text.to_lowercase())),
        (StringToUppercase, [_]) => Ok(Value::Str(text.to_uppercase())),
        (StringToAsciiLowercase | StringToAsciiUppercase, [_]) => {
            let Value::Str(mut result) = copy_string(text)? else {
                unreachable!()
            };
            if intrinsic == StringToAsciiLowercase {
                result.make_ascii_lowercase();
            } else {
                result.make_ascii_uppercase();
            }
            Ok(Value::Str(result))
        }
        (StringRepeat, [_, Value::U64(count)]) => {
            if text.is_empty() {
                return Ok(Value::Str(String::new()));
            }
            let count = usize::try_from(*count).map_err(|_| allocation())?;
            let size = text.len().checked_mul(count).ok_or_else(allocation)?;
            let mut result = String::new();
            result.try_reserve_exact(size).map_err(|_| allocation())?;
            for _ in 0..count {
                result.push_str(text);
            }
            Ok(Value::Str(result))
        }
        (StringReplace, [_, Value::Str(from), Value::Str(to)]) => {
            replace_string(text, from, to, usize::MAX)
        }
        (StringReplaceN, [_, Value::Str(from), Value::Str(to), Value::U64(count)]) => {
            replace_string(
                text,
                from,
                to,
                usize::try_from(*count).unwrap_or(usize::MAX),
            )
        }
        _ => Err(BuiltinError::new("invalid string operation arguments")),
    }
}

fn replace_string(text: &str, from: &str, to: &str, count: usize) -> Result<Value, BuiltinError> {
    let allocation = || {
        BuiltinError::from(crate::error::RuntimeError::resource_limit(
            "string replacement size",
        ))
    };
    let mut length = text.len();
    for (_, matched) in text.match_indices(from).take(count) {
        length = length
            .checked_sub(matched.len())
            .and_then(|n| n.checked_add(to.len()))
            .ok_or_else(allocation)?;
    }
    let mut output = String::new();
    output.try_reserve_exact(length).map_err(|_| allocation())?;
    let mut previous = 0;
    for (index, matched) in text.match_indices(from).take(count) {
        output.push_str(&text[previous..index]);
        output.push_str(to);
        previous = index + matched.len();
    }
    output.push_str(&text[previous..]);
    Ok(Value::Str(output))
}

fn string_split_once(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    let [Value::Str(text), Value::Str(separator)] = args else {
        return Err(BuiltinError::new("split_once requires two strings"));
    };
    let split = if intrinsic == StandardIntrinsic::StringSplitOnce {
        text.split_once(separator.as_str())
    } else {
        text.rsplit_once(separator.as_str())
    };
    match split {
        Some((left, right)) => option_some(
            gc,
            Value::Tuple(vec![copy_string(left)?, copy_string(right)?]),
        ),
        None => option_none(gc),
    }
}

fn string_query(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    use StandardIntrinsic::*;
    let Some(Value::Str(text)) = args.first() else {
        return Err(BuiltinError::new("string query requires a string receiver"));
    };
    if matches!(intrinsic, StringTrim | StringTrimStart | StringTrimEnd) {
        if args.len() != 1 {
            return Err(BuiltinError::new("trim expects one argument"));
        }
        return copy_string(match intrinsic {
            StringTrim => text.trim(),
            StringTrimStart => text.trim_start(),
            _ => text.trim_end(),
        });
    }
    let [_, Value::Str(needle)] = args else {
        return Err(BuiltinError::new("string query requires a string pattern"));
    };
    if matches!(intrinsic, StringFind | StringRfind) {
        let found = if intrinsic == StringFind {
            text.find(needle)
        } else {
            text.rfind(needle)
        };
        return match found {
            Some(index) => option_some(gc, Value::U64(index as u64)),
            None => option_none(gc),
        };
    }
    let stripped = if intrinsic == StringStripPrefix {
        text.strip_prefix(needle.as_str())
    } else {
        text.strip_suffix(needle.as_str())
    };
    match stripped {
        Some(value) => option_some(gc, copy_string(value)?),
        None => option_none(gc),
    }
}

fn copy_string(text: &str) -> Result<Value, BuiltinError> {
    let mut output = String::new();
    output.try_reserve_exact(text.len()).map_err(|_| {
        BuiltinError::from(crate::error::RuntimeError::resource_limit(
            "string allocation",
        ))
    })?;
    output.push_str(text);
    Ok(Value::Str(output))
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
    Ok(Value::Bool(option.tag == EnumTag::OptionSome))
}

fn option_is_none(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let option = option_value(gc, args, "option.is_none")?;
    Ok(Value::Bool(option.tag == EnumTag::OptionNone))
}

fn option_unwrap_or(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, fallback] = args else {
        return Err(BuiltinError::new(
            "option.unwrap_or expects option and fallback",
        ));
    };
    match option_snapshot(gc, value, "option.unwrap_or")?.tag {
        EnumTag::OptionSome => option_payload(gc, value, "option.unwrap_or"),
        EnumTag::OptionNone => Ok(fallback.clone()),
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
    match option_snapshot(gc, value, "option.map")?.tag {
        EnumTag::OptionSome => {
            let next = callbacks.call(callback, &[option_payload(gc, value, "option.map")?])?;
            option_some(gc, next)
        }
        EnumTag::OptionNone => option_none(gc),
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
    match option_snapshot(gc, value, "option.and_then")?.tag {
        EnumTag::OptionSome => {
            let next =
                callbacks.call(callback, &[option_payload(gc, value, "option.and_then")?])?;
            option_snapshot(gc, &next, "option.and_then mapper result")?;
            Ok(next)
        }
        EnumTag::OptionNone => option_none(gc),
        _ => unreachable!(),
    }
}

fn result_is_ok(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let result = result_value(gc, args, "result.is_ok")?;
    Ok(Value::Bool(result.tag == EnumTag::ResultOk))
}

fn result_is_err(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let result = result_value(gc, args, "result.is_err")?;
    Ok(Value::Bool(result.tag == EnumTag::ResultErr))
}

fn result_unwrap_or(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [value, fallback] = args else {
        return Err(BuiltinError::new(
            "result.unwrap_or expects result and fallback",
        ));
    };
    match result_snapshot(gc, value, "result.unwrap_or")?.tag {
        EnumTag::ResultOk => result_payload(gc, value, "result.unwrap_or"),
        EnumTag::ResultErr => Ok(fallback.clone()),
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
    match result_snapshot(gc, value, "result.map")?.tag {
        EnumTag::ResultOk => {
            let next = callbacks.call(callback, &[result_payload(gc, value, "result.map")?])?;
            result_ok(gc, next)
        }
        EnumTag::ResultErr => Ok(value.clone()),
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
    match result_snapshot(gc, value, "result.map_err")?.tag {
        EnumTag::ResultOk => result_ok(gc, result_payload(gc, value, "result.map_err")?),
        EnumTag::ResultErr => {
            let next = callbacks.call(callback, &[result_payload(gc, value, "result.map_err")?])?;
            gc.map_result_error(value, next)
                .map(Value::Enum)
                .map_err(Into::into)
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
    match result_snapshot(gc, value, "result.and_then")?.tag {
        EnumTag::ResultOk => {
            let next =
                callbacks.call(callback, &[result_payload(gc, value, "result.and_then")?])?;
            result_snapshot(gc, &next, "result.and_then mapper result")?;
            Ok(next)
        }
        EnumTag::ResultErr => Ok(value.clone()),
        _ => unreachable!(),
    }
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
        Value::I32(value) => kagari_common::arithmetic::i32_abs(*value)
            .map(Value::I32)
            .map_err(|error| BuiltinError::new(error.message())),
        Value::I64(value) => kagari_common::arithmetic::i64_abs(*value)
            .map(Value::I64)
            .map_err(|error| BuiltinError::new(error.message())),
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

fn debug_assert_eq(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [lhs, rhs, Value::Str(message)] = args else {
        return Err(BuiltinError::new(
            "debug.assert_eq expects two values and string message",
        ));
    };
    if crate::value_semantics::script_equal(gc, lhs, rhs)
        .map_err(|error| BuiltinError::new(error.to_string()))?
    {
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
        Value::U64(index) => {
            usize::try_from(*index).map_err(|_| BuiltinError::new("index exceeds platform range"))
        }
        _ => Err(BuiltinError::new(format!(
            "{name} expects non-negative integer index"
        ))),
    }
}

fn usize_value(value: usize) -> Value {
    Value::U64(value as u64)
}

fn require_hash_key(gc: &GcHeap, value: &Value, name: &'static str) -> Result<(), BuiltinError> {
    MapKey::from_value(gc, value).map(|_| ()).ok_or_else(|| {
        BuiltinError::new(format!(
            "{name} expects a valid Eq + Hash key within the key size limit"
        ))
    })
}

fn array_value(gc: &GcHeap, values: Vec<Value>) -> Result<Value, BuiltinError> {
    gc.alloc_array(values)
        .map(Value::Array)
        .map_err(BuiltinError::from)
}

fn option_some(gc: &GcHeap, value: Value) -> Result<Value, BuiltinError> {
    enum_value(gc, EnumTag::OptionSome, vec![value])
}

fn option_none(gc: &GcHeap) -> Result<Value, BuiltinError> {
    enum_value(gc, EnumTag::OptionNone, Vec::new())
}

fn result_ok(gc: &GcHeap, value: Value) -> Result<Value, BuiltinError> {
    enum_value(gc, EnumTag::ResultOk, vec![value])
}

fn result_err(gc: &GcHeap, value: Value) -> Result<Value, BuiltinError> {
    enum_value(gc, EnumTag::ResultErr, vec![value])
}

fn enum_value(gc: &GcHeap, tag: EnumTag, fields: Vec<Value>) -> Result<Value, BuiltinError> {
    gc.alloc_enum(tag, fields)
        .map(Value::Enum)
        .map_err(BuiltinError::from)
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
    match snapshot.tag {
        EnumTag::OptionSome | EnumTag::OptionNone => Ok(snapshot),
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
    match snapshot.tag {
        EnumTag::ResultOk | EnumTag::ResultErr => Ok(snapshot),
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
        (Value::U64(lhs), Value::U64(rhs)) => Ok(ordering_value(lhs.cmp(rhs))),
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

fn option_ok_or(
    gc: &GcHeap,
    args: &[Value],
    callbacks: &mut dyn BuiltinCallbacks,
    lazy: bool,
) -> Result<Value, BuiltinError> {
    let [value, error] = args else {
        return Err(BuiltinError::new(
            "option conversion expects option and error",
        ));
    };
    match option_snapshot(gc, value, "option.ok_or")?.tag {
        EnumTag::OptionSome => result_ok(gc, option_payload(gc, value, "option.ok_or")?),
        EnumTag::OptionNone => {
            let error = if lazy {
                callbacks.call(callback_id(error, "option.ok_or_else")?, &[])?
            } else {
                error.clone()
            };
            result_err(gc, error)
        }
        _ => unreachable!(),
    }
}

fn custom_key_operation(
    gc: &GcHeap,
    op: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    use StandardIntrinsic::*;
    let [collection, Value::I64(hash), Value::I64(token), rest @ ..] = args else {
        return Err(BuiltinError::new("invalid key operation arguments"));
    };
    if matches!(op, KeyMapGet | KeyMapInsert | KeyMapRemove) && !matches!(collection, Value::Map(_))
        || matches!(op, KeySetContains | KeySetInsert | KeySetRemove)
            && !matches!(collection, Value::Set(_))
    {
        return Err(BuiltinError::new("key operation collection category"));
    }
    match op {
        KeyMapGet | KeyMapRemove => {
            if op == KeyMapRemove
                && let Value::Map(id) = collection
            {
                gc.ensure_structure_mutable(*id)?;
            }
            let value = gc.custom_get(collection, *hash, *token)?;
            let result = match value {
                Some(value) => option_some(gc, value)?,
                None => option_none(gc)?,
            };
            if op == KeyMapRemove {
                gc.custom_remove(collection, *hash, *token)?;
            }
            Ok(result)
        }
        KeySetContains => Ok(Value::Bool(
            gc.custom_get(collection, *hash, *token)?.is_some(),
        )),
        KeyMapInsert | KeySetInsert => {
            let key = rest
                .first()
                .ok_or_else(|| BuiltinError::new("missing custom key"))?
                .clone();
            let value = if op == KeyMapInsert {
                rest.get(1)
                    .ok_or_else(|| BuiltinError::new("missing map value"))?
                    .clone()
            } else {
                Value::Unit
            };
            gc.custom_insert(collection, *hash, *token, key, value)?;
            Ok(collection.clone())
        }
        KeySetRemove => {
            let exists = gc.custom_get(collection, *hash, *token)?.is_some();
            gc.custom_remove(collection, *hash, *token)?;
            Ok(Value::Bool(exists))
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_ir::builtin::surface::StandardIntrinsic;

    use crate::{
        gc::{GcHeap, GcHeapConfig},
        value::EphemeralValue,
    };

    #[test]
    fn join_validates_native_arguments_and_leaves_the_array_unchanged() {
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        let handle = heap
            .alloc_array(vec![Value::Str("é".into()), Value::Str("😀".into())])
            .unwrap();
        let before = heap.array_snapshot(handle).unwrap();
        assert_eq!(
            array_join(&heap, &[Value::Array(handle), Value::Str("/".into())]).unwrap(),
            Value::Str("é/😀".into())
        );
        assert_eq!(heap.array_snapshot(handle).unwrap(), before);
        assert!(array_join(&heap, &[Value::I32(1), Value::Str("".into())]).is_err());
        let invalid = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        assert!(array_join(&heap, &[Value::Array(invalid), Value::Str("".into())]).is_err());
        assert_eq!(heap.array_snapshot(invalid).unwrap(), vec![Value::I32(1)]);
        assert!(
            array_join(
                &GcHeap::new(
                    GcHeapConfig::default(),
                    std::rc::Rc::new(crate::resource::ResourceState::default())
                ),
                &[Value::Array(handle), Value::Str("".into())]
            )
            .is_err()
        );
    }

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
        assert_eq!(snapshot.tag.type_name(), "Option");
        (snapshot.tag.variant_name().to_owned(), snapshot.fields)
    }

    fn result_variant(gc: &GcHeap, value: &Value) -> (String, Vec<Value>) {
        let Value::Enum(handle) = value else {
            panic!("expected enum value");
        };
        let snapshot = gc.enum_snapshot(*handle).unwrap();
        assert_eq!(snapshot.tag.type_name(), "Result");
        (snapshot.tag.variant_name().to_owned(), snapshot.fields)
    }

    #[test]
    fn builtin_standard_array_helpers_mutate_and_return_options() {
        let gc = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        let array = Value::Array(gc.alloc_array(vec![Value::I32(1)]).unwrap());

        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::ArrayLen,
                std::slice::from_ref(&array)
            )
            .unwrap(),
            Value::U64(1)
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
            &[array.clone(), Value::U64(1), Value::I32(2)],
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
        let gc = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        let map = call(&gc, StandardIntrinsic::LinkedHashMapNew, &[]).unwrap();
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
        let keys = call(
            &gc,
            StandardIntrinsic::MapKeysStorage,
            std::slice::from_ref(&map),
        )
        .unwrap();
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
    fn builtin_standard_string_helpers_validate_utf8_boundaries() {
        let gc = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::StringLenBytes,
                &[Value::Str("éx".to_owned())]
            )
            .unwrap(),
            Value::U64(3)
        );
        assert_eq!(
            call(
                &gc,
                StandardIntrinsic::StringLenChars,
                &[Value::Str("éx".to_owned())]
            )
            .unwrap(),
            Value::U64(2)
        );
        let good = call(
            &gc,
            StandardIntrinsic::StringSlice,
            &[Value::Str("éx".to_owned()), Value::U64(0), Value::U64(2)],
        )
        .unwrap();
        assert_eq!(
            option_variant(&gc, &good),
            ("Some".to_owned(), vec![Value::Str("é".to_owned())])
        );
        let bad = call(
            &gc,
            StandardIntrinsic::StringSlice,
            &[Value::Str("éx".to_owned()), Value::U64(1), Value::U64(2)],
        )
        .unwrap();
        assert_eq!(option_variant(&gc, &bad), ("None".to_owned(), Vec::new()));
    }

    #[test]
    fn builtin_standard_option_result_helpers_use_standard_enum_values() {
        let gc = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
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
    fn builtin_standard_math_and_debug_helpers_are_deterministic() {
        let gc = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
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
    #[test]
    fn collection_iteration_rejects_structural_alias_writes_before_allocation() {
        let gc = GcHeap::new(Default::default(), Default::default());
        let array = Value::Array(gc.alloc_array(vec![Value::I32(1)]).unwrap());
        let map = Value::Map(gc.alloc_map(vec![(Value::I32(1), Value::I32(2))]).unwrap());
        let set = Value::Set(gc.alloc_set(vec![Value::I32(1)]).unwrap());
        let operations = [
            (
                StandardIntrinsic::ArrayPush,
                vec![array.clone(), Value::I32(2)],
            ),
            (StandardIntrinsic::ArrayPop, vec![array.clone()]),
            (
                StandardIntrinsic::ArrayInsert,
                vec![array.clone(), Value::I32(0), Value::I32(2)],
            ),
            (
                StandardIntrinsic::ArrayRemove,
                vec![array.clone(), Value::I32(0)],
            ),
            (StandardIntrinsic::ArrayClear, vec![array.clone()]),
            (
                StandardIntrinsic::MapInsert,
                vec![map.clone(), Value::I32(3), Value::I32(4)],
            ),
            (
                StandardIntrinsic::MapRemove,
                vec![map.clone(), Value::I32(1)],
            ),
            (StandardIntrinsic::MapClear, vec![map.clone()]),
            (
                StandardIntrinsic::SetInsert,
                vec![set.clone(), Value::I32(2)],
            ),
            (
                StandardIntrinsic::SetRemove,
                vec![set.clone(), Value::I32(1)],
            ),
            (StandardIntrinsic::SetClear, vec![set.clone()]),
        ];
        for (op, args) in operations {
            let snapshot = || match &args[0] {
                Value::Array(id) => gc.array_snapshot(*id).unwrap(),
                Value::Set(id) => gc.set_snapshot(*id).unwrap(),
                Value::Map(id) => gc
                    .map_snapshot(*id)
                    .unwrap()
                    .into_iter()
                    .map(|(k, v)| Value::Tuple(vec![k, v]))
                    .collect(),
                _ => unreachable!(),
            };
            let guard = gc.begin_collection_iteration(&args[0]).unwrap();
            let before = snapshot();
            let units = gc.stats().allocation_units;
            let error = invoke(&gc, op, &args).unwrap_err();
            assert_eq!(error.message(), "structural modification during iteration");
            assert_eq!(snapshot(), before);
            assert_eq!(gc.stats().allocation_units, units);
            drop(guard);
        }
        let Value::Array(id) = array else {
            unreachable!()
        };
        let guard = gc.begin_collection_iteration(&Value::Array(id)).unwrap();
        gc.array_set(id, 0, Value::I32(9)).unwrap();
        let nested = gc.begin_collection_iteration(&Value::Array(id)).unwrap();
        drop(nested);
        assert!(gc.array_push(id, Value::I32(2)).is_err());
        drop(guard);
        gc.array_push(id, Value::I32(2)).unwrap();
        let guard = gc.begin_collection_iteration(&map).unwrap();
        invoke(
            &gc,
            StandardIntrinsic::MapInsert,
            &[map, Value::I32(1), Value::I32(9)],
        )
        .unwrap();
        drop(guard);
        let guard = gc.begin_collection_iteration(&set).unwrap();
        invoke(&gc, StandardIntrinsic::SetInsert, &[set, Value::I32(1)]).unwrap();
        drop(guard);
    }
}
