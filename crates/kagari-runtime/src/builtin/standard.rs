use crate::builtin::standard::math::{
    math_abs, math_clamp, math_max, math_min, math_sqrt, math_unary_f64,
};
mod math;
use crate::builtin::standard::strings::{
    string_concat, string_contains, string_ends_with, string_is_empty, string_len_bytes,
    string_len_chars, string_query, string_slice, string_split_once, string_starts_with,
    string_transform,
};
mod strings;
use crate::{
    builtin::BuiltinError,
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId},
    numeric, parsing, range,
    value::{EnumTag, EnumValueSnapshot, EphemeralValue, EphemeralValueId, MapKey, Value},
    value_semantics,
};
use kagari_abi::standard::StandardIntrinsic;
use std::cmp::Ordering;

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
    match intrinsic {
        StandardIntrinsic::MapContainsKey
        | StandardIntrinsic::MapGet
        | StandardIntrinsic::MapInsert
        | StandardIntrinsic::MapRemove
        | StandardIntrinsic::SetContains
        | StandardIntrinsic::SetInsert
        | StandardIntrinsic::SetRemove => {
            if let Some(collection) = args.first() {
                gc.ensure_key_mode(collection, false)?;
            }
        }

        _ => {}
    }
    match intrinsic {
        StandardIntrinsic::ArrayReplaceStorage | StandardIntrinsic::CollectionRetainStorage => {
            gc.commit_prepared_collection(intrinsic, args)?;
            Ok(Value::Unit)
        }
        StandardIntrinsic::ArrayRetain
        | StandardIntrinsic::MapRetain
        | StandardIntrinsic::SetRetain
        | StandardIntrinsic::ArraySort
        | StandardIntrinsic::ArraySortBy
        | StandardIntrinsic::ArraySortByKey
        | StandardIntrinsic::ArrayDedup => Err(BuiltinError::new(
            "collection callback requires static lowering",
        )),
        StandardIntrinsic::IterResume => {
            gc.resume_iter(
                args.first()
                    .ok_or_else(|| BuiltinError::new("missing iterator"))?,
            )?;
            Ok(Value::Unit)
        }
        StandardIntrinsic::StringParse => Err(BuiltinError::new("parse requires static dispatch")),
        StandardIntrinsic::ParseNumber(ty) => {
            parsing::parse(gc, ty, args, false).map_err(Into::into)
        }
        StandardIntrinsic::ParseRadix(ty) => parsing::parse(gc, ty, args, true).map_err(Into::into),
        StandardIntrinsic::Integer(operation, ty) => {
            numeric::integer_method(gc, operation, ty, args).map_err(Into::into)
        }
        StandardIntrinsic::CollectionMutationBegin
        | StandardIntrinsic::CollectionMutationEnd
        | StandardIntrinsic::MapGetOrInsertWith
        | StandardIntrinsic::MapUpdate => Err(BuiltinError::new(
            "collection operation requires static lowering",
        )),
        StandardIntrinsic::KeyLookupBegin => {
            Err(BuiltinError::new("key lookup requires an execution frame"))
        }
        StandardIntrinsic::KeyCandidates => {
            let [collection, Value::I64(hash)] = args else {
                return Err(BuiltinError::new("invalid key candidates arguments"));
            };
            array_value(gc, gc.custom_candidates(collection, *hash)?)
        }
        StandardIntrinsic::KeyMapGet
        | StandardIntrinsic::KeyMapInsert
        | StandardIntrinsic::KeyMapRemove
        | StandardIntrinsic::KeySetContains
        | StandardIntrinsic::KeySetInsert
        | StandardIntrinsic::KeySetRemove => custom_key_operation(gc, intrinsic, args),
        StandardIntrinsic::ValuePartialCmp | StandardIntrinsic::ValueCmp => {
            let [a, b] = args else {
                return Err(BuiltinError::new("comparison requires two operands"));
            };
            let ordering = value_semantics::builtin_order(gc, a, b)?;
            let Some(ordering) = ordering else {
                if intrinsic == StandardIntrinsic::ValueCmp {
                    return Err(BuiltinError::new("total comparison cannot be unordered"));
                }
                return option_none(gc);
            };
            let tag = match ordering {
                Ordering::Less => EnumTag::OrderingLess,
                Ordering::Equal => EnumTag::OrderingEqual,
                Ordering::Greater => EnumTag::OrderingGreater,
            };
            let value = Value::Enum(gc.alloc_enum(tag, vec![])?);
            if intrinsic == StandardIntrinsic::ValuePartialCmp {
                option_some(gc, value)
            } else {
                Ok(value)
            }
        }
        StandardIntrinsic::ValueEq => {
            let [a, b] = args else {
                return Err(BuiltinError::new("eq expects two operands"));
            };
            value_semantics::script_equal(gc, a, b)
                .map(Value::Bool)
                .map_err(BuiltinError::from)
        }
        StandardIntrinsic::ValueHash => {
            let [value] = args else {
                return Err(BuiltinError::new("hash expects one operand"));
            };
            MapKey::from_value(gc, value)
                .map(|key| Value::I64(key.script_hash()))
                .ok_or_else(|| {
                    BuiltinError::new("value has no hash semantics or exceeds key size limit")
                })
        }
        StandardIntrinsic::ValueDebug | StandardIntrinsic::ValueDisplay => {
            let [value] = args else {
                return Err(BuiltinError::new("format expects one operand"));
            };
            value_semantics::format_value(gc, value, intrinsic == StandardIntrinsic::ValueDebug)
                .map(Value::Str)
                .map_err(BuiltinError::from)
        }
        StandardIntrinsic::ArrayListNew => {
            if !args.is_empty() {
                return Err(BuiltinError::new("new expects no arguments"));
            }
            Ok(Value::Array(gc.alloc_array(vec![])?))
        }
        StandardIntrinsic::MapKeys
        | StandardIntrinsic::MapValues
        | StandardIntrinsic::MapEntries
        | StandardIntrinsic::ArrayCopyFrom
        | StandardIntrinsic::ArrayListFromFn
        | StandardIntrinsic::ArrayListFrom
        | StandardIntrinsic::LinkedHashMapFrom
        | StandardIntrinsic::LinkedHashSetFrom => Err(BuiltinError::new(
            "collection factories must be lowered to checked construction",
        )),
        StandardIntrinsic::ArrayLen => array_len(gc, args),
        StandardIntrinsic::ArrayIsEmpty => array_is_empty(gc, args),
        StandardIntrinsic::ArrayGet => array_get(gc, args),
        StandardIntrinsic::ArrayPush => array_push(gc, args),
        StandardIntrinsic::ArrayPop => array_pop(gc, args),
        StandardIntrinsic::ArrayInsert => array_insert(gc, args),
        StandardIntrinsic::ArrayRemove => array_remove(gc, args),
        StandardIntrinsic::ArrayWithCapacity
        | StandardIntrinsic::ArrayCapacity
        | StandardIntrinsic::ArrayReserve
        | StandardIntrinsic::MapWithCapacity
        | StandardIntrinsic::MapCapacity
        | StandardIntrinsic::MapReserve
        | StandardIntrinsic::SetWithCapacity
        | StandardIntrinsic::SetCapacity
        | StandardIntrinsic::SetReserve => collection_capacity(gc, intrinsic, args),
        StandardIntrinsic::ArrayExtend => Err(BuiltinError::new(
            "extend requires prepared source lowering",
        )),
        StandardIntrinsic::ArraySwap
        | StandardIntrinsic::ArrayReverse
        | StandardIntrinsic::ArrayTruncate
        | StandardIntrinsic::ArrayExtendStorage
        | StandardIntrinsic::ArraySwapRemove => array_mutation(gc, intrinsic, args),
        StandardIntrinsic::ArrayJoin => array_join(gc, args),
        StandardIntrinsic::ArrayClear => array_clear(gc, args),
        StandardIntrinsic::ArrayRemoveRangePrepare => {
            let [Value::Array(target), start, end] = args else {
                return Err(BuiltinError::new("invalid remove range operands"));
            };
            let start = range::index_bound(gc, start)?;
            let end = range::index_bound(gc, end)?;
            gc.prepare_array_removal(*target, start, end)
                .map_err(Into::into)
        }
        StandardIntrinsic::ArrayRemoveRange | StandardIntrinsic::ArrayCopyWithin => {
            Err(BuiltinError::new("range bounds require static lowering"))
        }
        StandardIntrinsic::ArrayCopyWithinBounds => {
            let [Value::Array(target), start, end, destination] = args else {
                return Err(BuiltinError::new("invalid copy_within operands"));
            };
            let destination = usize::try_from(match destination {
                Value::U64(n) => *n,
                _ => return Err(BuiltinError::new("invalid copy destination")),
            })
            .map_err(|_| BuiltinError::new("copy destination exceeds platform capacity"))?;
            let start = range::index_bound(gc, start)?;
            let end = range::index_bound(gc, end)?;
            gc.array_copy_within(*target, start, end, destination)?;
            Ok(Value::Unit)
        }
        StandardIntrinsic::ArrayFill => {
            let [Value::Array(target), value] = args else {
                return Err(BuiltinError::new("array.fill expects an array and value"));
            };
            gc.array_fill(*target, value.clone())?;
            Ok(Value::Unit)
        }
        StandardIntrinsic::ArrayCopyFromStorage => {
            let [Value::Array(target), Value::Array(source)] = args else {
                return Err(BuiltinError::new("array.copy_from expects two arrays"));
            };
            gc.array_copy_from(*target, *source)?;
            Ok(Value::Unit)
        }
        StandardIntrinsic::LinkedHashMapNew => map_new(gc, args),
        StandardIntrinsic::MapLen => map_len(gc, args),
        StandardIntrinsic::MapIsEmpty => map_is_empty(gc, args),
        StandardIntrinsic::MapContainsKey => map_contains_key(gc, args),
        StandardIntrinsic::MapGet => map_get(gc, args),
        StandardIntrinsic::MapInsert => map_insert(gc, args),
        StandardIntrinsic::MapRemove => map_remove(gc, args),
        StandardIntrinsic::MapClear => map_clear(gc, args),
        StandardIntrinsic::MapKeysStorage => map_keys(gc, args),
        StandardIntrinsic::MapValuesStorage => map_values(gc, args),
        StandardIntrinsic::MapEntriesStorage => map_entries(gc, args),
        StandardIntrinsic::LinkedHashSetNew => set_new(gc, args),
        StandardIntrinsic::SetLen => set_len(gc, args),
        StandardIntrinsic::SetIsEmpty => set_is_empty(gc, args),
        StandardIntrinsic::SetContains => set_contains(gc, args),
        StandardIntrinsic::SetInsert => set_insert(gc, args),
        StandardIntrinsic::SetRemove => set_remove(gc, args),
        StandardIntrinsic::SetClear => set_clear(gc, args),
        StandardIntrinsic::SetToArray => set_to_array(gc, args),
        StandardIntrinsic::StringLenBytes => string_len_bytes(args),
        StandardIntrinsic::StringLenChars => string_len_chars(args),
        StandardIntrinsic::StringIsEmpty => string_is_empty(args),
        StandardIntrinsic::StringConcat => string_concat(args),
        StandardIntrinsic::StringContains => string_contains(args),
        StandardIntrinsic::StringStartsWith => string_starts_with(args),
        StandardIntrinsic::StringEndsWith => string_ends_with(args),
        StandardIntrinsic::StringSlice => string_slice(gc, args),
        StandardIntrinsic::StringReplace
        | StandardIntrinsic::StringReplaceN
        | StandardIntrinsic::StringRepeat
        | StandardIntrinsic::StringIsAscii
        | StandardIntrinsic::StringEqIgnoreAsciiCase
        | StandardIntrinsic::StringToAsciiLowercase
        | StandardIntrinsic::StringToAsciiUppercase
        | StandardIntrinsic::StringToLowercase
        | StandardIntrinsic::StringToUppercase
        | StandardIntrinsic::StringIsCharBoundary => string_transform(intrinsic, args),
        StandardIntrinsic::StringSplit
        | StandardIntrinsic::StringSplitN
        | StandardIntrinsic::StringSplitWhitespace
        | StandardIntrinsic::StringLines
        | StandardIntrinsic::StringBytes
        | StandardIntrinsic::StringCharIndices => Err(BuiltinError::new(
            "string traversal requires iterator lowering",
        )),
        StandardIntrinsic::StringSplitOnce | StandardIntrinsic::StringRsplitOnce => {
            string_split_once(gc, intrinsic, args)
        }
        StandardIntrinsic::StringTrim
        | StandardIntrinsic::StringTrimStart
        | StandardIntrinsic::StringTrimEnd
        | StandardIntrinsic::StringFind
        | StandardIntrinsic::StringRfind
        | StandardIntrinsic::StringStripPrefix
        | StandardIntrinsic::StringStripSuffix => string_query(gc, intrinsic, args),
        StandardIntrinsic::OptionUnwrapOrElse
        | StandardIntrinsic::OptionOrElse
        | StandardIntrinsic::OptionMapOr
        | StandardIntrinsic::OptionMapOrElse
        | StandardIntrinsic::OptionFilter
        | StandardIntrinsic::OptionIsSomeAnd
        | StandardIntrinsic::OptionZip
        | StandardIntrinsic::OptionFlatten
        | StandardIntrinsic::OptionTranspose
        | StandardIntrinsic::ResultUnwrapOrElse
        | StandardIntrinsic::ResultOrElse
        | StandardIntrinsic::ResultMapOr
        | StandardIntrinsic::ResultMapOrElse
        | StandardIntrinsic::ResultOk
        | StandardIntrinsic::ResultErr
        | StandardIntrinsic::ResultIsOkAnd
        | StandardIntrinsic::ResultIsErrAnd
        | StandardIntrinsic::ResultFlatten
        | StandardIntrinsic::ResultTranspose => {
            Err(BuiltinError::new("enum combinators require frame lowering"))
        }
        StandardIntrinsic::OptionIsSome => option_is_some(gc, args),
        StandardIntrinsic::OptionIsNone => option_is_none(gc, args),
        StandardIntrinsic::OptionUnwrapOr => option_unwrap_or(gc, args),
        StandardIntrinsic::OptionMap => option_map(gc, args, callbacks),
        StandardIntrinsic::OptionAndThen => option_and_then(gc, args, callbacks),
        StandardIntrinsic::OptionOkOr | StandardIntrinsic::OptionOkOrElse => option_ok_or(
            gc,
            args,
            callbacks,
            intrinsic == StandardIntrinsic::OptionOkOrElse,
        ),
        StandardIntrinsic::ResultIsOk => result_is_ok(gc, args),
        StandardIntrinsic::ResultIsErr => result_is_err(gc, args),
        StandardIntrinsic::ResultUnwrapOr => result_unwrap_or(gc, args),
        StandardIntrinsic::ResultMap => result_map(gc, args, callbacks),
        StandardIntrinsic::ResultMapErr => result_map_err(gc, args, callbacks),
        StandardIntrinsic::ResultAndThen => result_and_then(gc, args, callbacks),
        StandardIntrinsic::MathMin => math_min(args),
        StandardIntrinsic::MathMax => math_max(args),
        StandardIntrinsic::MathClamp => math_clamp(args),
        StandardIntrinsic::MathAbs => math_abs(args),
        StandardIntrinsic::MathFloor => math_unary_f64(args, "math.floor", f64::floor),
        StandardIntrinsic::MathCeil => math_unary_f64(args, "math.ceil", f64::ceil),
        StandardIntrinsic::MathRound => math_unary_f64(args, "math.round", f64::round),
        StandardIntrinsic::MathSqrt => math_sqrt(args),
        StandardIntrinsic::MathSin => math_unary_f64(args, "math.sin", f64::sin),
        StandardIntrinsic::MathCos => math_unary_f64(args, "math.cos", f64::cos),
        StandardIntrinsic::MathTan => math_unary_f64(args, "math.tan", f64::tan),
        StandardIntrinsic::DebugPrint => debug_print(args),
        StandardIntrinsic::DebugAssert => debug_assert(args),
        StandardIntrinsic::DebugAssertEq => debug_assert_eq(gc, args),
        StandardIntrinsic::DebugPanic => debug_panic(args),
    }
}

fn collection_capacity(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    if matches!(
        intrinsic,
        StandardIntrinsic::ArrayWithCapacity
            | StandardIntrinsic::MapWithCapacity
            | StandardIntrinsic::SetWithCapacity
    ) {
        let [Value::U64(capacity)] = args else {
            return Err(BuiltinError::new("capacity constructor requires usize"));
        };
        let capacity = usize::try_from(*capacity)
            .map_err(|_| BuiltinError::new("capacity exceeds platform limit"))?;
        let value = match intrinsic {
            StandardIntrinsic::ArrayWithCapacity => Value::Array(gc.alloc_array(vec![])?),
            StandardIntrinsic::MapWithCapacity => Value::Map(gc.alloc_map(vec![])?),
            _ => Value::Set(gc.alloc_set(vec![])?),
        };
        gc.reserve_collection(&value, capacity)?;
        return Ok(value);
    }
    match args {
        [value]
            if matches!(
                intrinsic,
                StandardIntrinsic::ArrayCapacity
                    | StandardIntrinsic::MapCapacity
                    | StandardIntrinsic::SetCapacity
            ) =>
        {
            Ok(Value::U64(gc.collection_capacity(value)? as u64))
        }
        [value, Value::U64(additional)]
            if matches!(
                intrinsic,
                StandardIntrinsic::ArrayReserve
                    | StandardIntrinsic::MapReserve
                    | StandardIntrinsic::SetReserve
            ) =>
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
        (StandardIntrinsic::ArraySwap, [_, a, b]) => gc.array_swap(*id, index(a)?, index(b)?)?,
        (StandardIntrinsic::ArrayReverse, [_]) => gc.array_reverse(*id)?,
        (StandardIntrinsic::ArrayTruncate, [_, len]) => gc.array_truncate(*id, index(len)?)?,
        (StandardIntrinsic::ArrayExtendStorage, [_, Value::Array(source)]) => {
            gc.array_extend(*id, *source)?
        }
        (StandardIntrinsic::ArraySwapRemove, [_, position]) => {
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
        let overflow = || BuiltinError::from(RuntimeError::resource_limit("joined string size"));
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
    if value_semantics::script_equal(gc, lhs, rhs)
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

fn one_array(args: &[Value], name: &'static str) -> Result<HeapObjectId, BuiltinError> {
    let [Value::Array(handle)] = args else {
        return Err(BuiltinError::new(format!("{name} expects one array")));
    };
    Ok(*handle)
}

fn one_map(args: &[Value], name: &'static str) -> Result<HeapObjectId, BuiltinError> {
    let [Value::Map(handle)] = args else {
        return Err(BuiltinError::new(format!("{name} expects one map")));
    };
    Ok(*handle)
}

fn one_set(args: &[Value], name: &'static str) -> Result<HeapObjectId, BuiltinError> {
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
) -> Result<EnumValueSnapshot, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new(format!("{name} expects one option")));
    };
    option_snapshot(gc, value, name)
}

fn result_value(
    gc: &GcHeap,
    args: &[Value],
    name: &'static str,
) -> Result<EnumValueSnapshot, BuiltinError> {
    let [value] = args else {
        return Err(BuiltinError::new(format!("{name} expects one result")));
    };
    result_snapshot(gc, value, name)
}

fn option_snapshot(
    gc: &GcHeap,
    value: &Value,
    name: &'static str,
) -> Result<EnumValueSnapshot, BuiltinError> {
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
) -> Result<EnumValueSnapshot, BuiltinError> {
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
    let [collection, Value::I64(hash), Value::I64(token), rest @ ..] = args else {
        return Err(BuiltinError::new("invalid key operation arguments"));
    };
    if matches!(
        op,
        StandardIntrinsic::KeyMapGet
            | StandardIntrinsic::KeyMapInsert
            | StandardIntrinsic::KeyMapRemove
    ) && !matches!(collection, Value::Map(_))
        || matches!(
            op,
            StandardIntrinsic::KeySetContains
                | StandardIntrinsic::KeySetInsert
                | StandardIntrinsic::KeySetRemove
        ) && !matches!(collection, Value::Set(_))
    {
        return Err(BuiltinError::new("key operation collection category"));
    }
    match op {
        StandardIntrinsic::KeyMapGet | StandardIntrinsic::KeyMapRemove => {
            if op == StandardIntrinsic::KeyMapRemove
                && let Value::Map(id) = collection
            {
                gc.ensure_structure_mutable(*id)?;
            }
            let value = gc.custom_get(collection, *hash, *token)?;
            let result = match value {
                Some(value) => option_some(gc, value)?,
                None => option_none(gc)?,
            };
            if op == StandardIntrinsic::KeyMapRemove {
                gc.custom_remove(collection, *hash, *token)?;
            }
            Ok(result)
        }
        StandardIntrinsic::KeySetContains => Ok(Value::Bool(
            gc.custom_get(collection, *hash, *token)?.is_some(),
        )),
        StandardIntrinsic::KeyMapInsert | StandardIntrinsic::KeySetInsert => {
            let key = rest
                .first()
                .ok_or_else(|| BuiltinError::new("missing custom key"))?
                .clone();
            let value = if op == StandardIntrinsic::KeyMapInsert {
                rest.get(1)
                    .ok_or_else(|| BuiltinError::new("missing map value"))?
                    .clone()
            } else {
                Value::Unit
            };
            gc.custom_insert(collection, *hash, *token, key, value)?;
            Ok(collection.clone())
        }
        StandardIntrinsic::KeySetRemove => {
            let exists = gc.custom_get(collection, *hash, *token)?.is_some();
            gc.custom_remove(collection, *hash, *token)?;
            Ok(Value::Bool(exists))
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests;
