//! Runtime implementations of language primitives; standard algorithms belong to providers.
use crate::{
    builtin::BuiltinError,
    error::RuntimeError,
    gc::GcHeap,
    native::sequence::SequenceStorage,
    value::{EnumTag, MapKey, Value},
    value_semantics,
};
use kagari_contract::standard::RuntimePrimitive;
use std::cmp::Ordering;
#[cfg(test)]
mod tests;

pub fn invoke(
    gc: &GcHeap,
    primitive: RuntimePrimitive,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    let intrinsic = primitive;
    match primitive {
        RuntimePrimitive::ValuePartialCmp | RuntimePrimitive::ValueCmp => {
            let [a, b] = args else {
                return Err(BuiltinError::new("comparison requires two operands"));
            };
            let ordering = value_semantics::builtin_order(gc, a, b)?;
            let Some(ordering) = ordering else {
                if intrinsic == RuntimePrimitive::ValueCmp {
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
            if intrinsic == RuntimePrimitive::ValuePartialCmp {
                option_some(gc, value)
            } else {
                Ok(value)
            }
        }
        RuntimePrimitive::ValueEq => {
            let [a, b] = args else {
                return Err(BuiltinError::new("eq expects two operands"));
            };
            value_semantics::script_equal(gc, a, b)
                .map(Value::Bool)
                .map_err(BuiltinError::from)
        }
        RuntimePrimitive::ValueHash => {
            let [value] = args else {
                return Err(BuiltinError::new("hash expects one operand"));
            };
            MapKey::from_value(gc, value)
                .map(|key| Value::I64(key.script_hash()))
                .ok_or_else(|| {
                    BuiltinError::new("value has no hash semantics or exceeds key size limit")
                })
        }
        RuntimePrimitive::ValueDebug | RuntimePrimitive::ValueDisplay => {
            let [value] = args else {
                return Err(BuiltinError::new("format expects one operand"));
            };
            value_semantics::format_value(gc, value, intrinsic == RuntimePrimitive::ValueDebug)
                .map(Value::Str)
                .map_err(BuiltinError::from)
        }
        RuntimePrimitive::StringPartsJoin => array_join(gc, args),
        RuntimePrimitive::Assert => debug_assert(args),
    }
}

fn array_join(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Array(handle), Value::Str(separator)] = args else {
        return Err(BuiltinError::new(
            "array.join expects a string array and separator",
        ));
    };
    gc.with_array(*handle, |values| {
        let SequenceStorage::Traced(values) = values else {
            return Err(BuiltinError::new("array.join expects string elements"));
        };
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

fn option_some(gc: &GcHeap, value: Value) -> Result<Value, BuiltinError> {
    enum_value(gc, EnumTag::OptionSome, vec![value])
}

fn option_none(gc: &GcHeap) -> Result<Value, BuiltinError> {
    enum_value(gc, EnumTag::OptionNone, Vec::new())
}

fn enum_value(gc: &GcHeap, tag: EnumTag, fields: Vec<Value>) -> Result<Value, BuiltinError> {
    gc.alloc_enum(tag, fields)
        .map(Value::Enum)
        .map_err(BuiltinError::from)
}
