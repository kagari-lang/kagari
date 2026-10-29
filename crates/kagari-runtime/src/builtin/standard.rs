//! Runtime implementations of language primitives; standard algorithms belong to providers.
use crate::{
    Runtime,
    builtin::BuiltinError,
    error::RuntimeError,
    gc::GcHeap,
    gc::array_storage::ArrayStorage,
    module::{ExecutableModule, linked_execution::builtin_results::BuiltinResult},
    value::Value,
    value_semantics::{self, hash::script_hash},
};
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::cmp::Ordering;
#[cfg(test)]
mod tests;

pub(crate) fn invoke(
    runtime: &Runtime,
    owner: &ExecutableModule,
    primitive: RuntimePrimitive,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    let gc = runtime.gc();
    let intrinsic = primitive;
    match primitive {
        RuntimePrimitive::ValuePartialCmp | RuntimePrimitive::ValueCmp => {
            let [a, b] = args else {
                return Err(BuiltinError::new("comparison requires two operands"));
            };
            let ordering = value_semantics::builtin_order(gc, a, b)?;
            if intrinsic == RuntimePrimitive::ValueCmp && ordering.is_none() {
                return Err(BuiltinError::new("total comparison cannot be unordered"));
            }
            let applied = runtime.builtin_result_type(owner, BuiltinResult::Ordering)?;
            let value = ordering
                .map(|ordering| {
                    runtime.make_enum_member(
                        owner,
                        &applied,
                        match ordering {
                            Ordering::Less => "Less",
                            Ordering::Equal => "Equal",
                            Ordering::Greater => "Greater",
                        },
                        vec![],
                    )
                })
                .transpose()?;
            if intrinsic == RuntimePrimitive::ValuePartialCmp {
                let _root = value.as_ref().and_then(|value| gc.root_value(*value));
                let option = runtime.builtin_result_type(owner, BuiltinResult::OptionalOrdering)?;
                runtime
                    .make_enum_member(
                        owner,
                        &option,
                        if value.is_some() { "Some" } else { "None" },
                        value.into_iter().collect(),
                    )
                    .map_err(Into::into)
            } else {
                value.ok_or_else(|| BuiltinError::new("total comparison cannot be unordered"))
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
            script_hash(gc, value).map(Value::I64).ok_or_else(|| {
                BuiltinError::new("value has no hash semantics or exceeds key size limit")
            })
        }
        RuntimePrimitive::ValueDebug | RuntimePrimitive::ValueDisplay => {
            let [value] = args else {
                return Err(BuiltinError::new("format expects one operand"));
            };
            value_semantics::format_value(gc, value, intrinsic == RuntimePrimitive::ValueDebug)
                .and_then(|text| gc.alloc_string(text))
                .map_err(BuiltinError::from)
        }
        RuntimePrimitive::StringPartsJoin => array_join(gc, args),
        RuntimePrimitive::Assert => debug_assert(gc, args),
    }
}

fn array_join(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Array(handle), Value::Str(separator)] = args else {
        return Err(BuiltinError::new(
            "array.join expects a string array and separator",
        ));
    };
    if !gc
        .array_contract(*handle)
        .is_some_and(|contract| *contract.ty() == Ty::Builtin(BuiltinType::String))
    {
        return Err(BuiltinError::new(
            "string interpolation requires [String] storage",
        ));
    }
    let output = gc
        .with_buffer(*handle, |values| {
            let ArrayStorage::Traced(values) = values else {
                return Err(BuiltinError::new("array.join expects string elements"));
            };
            let overflow =
                || BuiltinError::from(RuntimeError::resource_limit("joined string size"));
            let separator = gc
                .string(*separator)
                .ok_or_else(|| BuiltinError::new("invalid string separator"))?;
            let mut length = separator
                .len()
                .checked_mul(values.len().saturating_sub(1))
                .ok_or_else(overflow)?;
            for value in values {
                let Value::Str(value) = value.value_ref() else {
                    return Err(BuiltinError::new("array.join expects string elements"));
                };
                let value = gc
                    .string(*value)
                    .ok_or_else(|| BuiltinError::new("invalid string element"))?;
                length = length.checked_add(value.len()).ok_or_else(overflow)?;
            }
            let mut output = String::new();
            output.try_reserve_exact(length).map_err(|_| overflow())?;
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push_str(&separator);
                }
                let Value::Str(value) = value.value_ref() else {
                    unreachable!("validated string element");
                };
                let value = gc
                    .string(*value)
                    .ok_or_else(|| BuiltinError::new("invalid string element"))?;
                output.push_str(&value);
            }
            Ok(output)
        })
        .ok_or_else(|| BuiltinError::new("array.join expects a valid array handle"))??;
    gc.alloc_string(output).map_err(Into::into)
}

fn debug_assert(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Bool(condition), Value::Str(message)] = args else {
        return Err(BuiltinError::new(
            "debug.assert expects bool and string message",
        ));
    };
    if *condition {
        Ok(Value::Unit)
    } else {
        let message = gc
            .string(*message)
            .ok_or_else(|| BuiltinError::new("invalid assertion message"))?;
        Err(BuiltinError::new(format!(
            "debug.assert failed: {}",
            &*message
        )))
    }
}
