//! Runtime implementations of language primitives; standard algorithms belong to providers.
use crate::{
    Runtime,
    builtin::BuiltinError,
    error::{RuntimeError, trap::TrapReason},
    gc::GcHeap,
    module::{ExecutableModule, linked_execution::builtin_results::BuiltinResult},
    native::sequence::SequenceStorage,
    value::{MapKey, Value},
    value_semantics,
};

use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::cmp::Ordering;
#[cfg(test)]
mod tests;

pub fn invoke(
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
                return Err(BuiltinError::new(TrapReason::ComparisonRequiresTwoOperands));
            };
            let ordering = value_semantics::builtin_order(gc, a, b)?;
            if intrinsic == RuntimePrimitive::ValueCmp && ordering.is_none() {
                return Err(BuiltinError::new(
                    TrapReason::TotalComparisonCannotBeUnordered,
                ));
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
                value.ok_or_else(|| BuiltinError::new(TrapReason::TotalComparisonCannotBeUnordered))
            }
        }

        RuntimePrimitive::ValueEq => {
            let [a, b] = args else {
                return Err(BuiltinError::new(TrapReason::EqExpectsTwoOperands));
            };
            value_semantics::script_equal(gc, a, b)
                .map(Value::Bool)
                .map_err(BuiltinError::from)
        }
        RuntimePrimitive::ValueHash => {
            let [value] = args else {
                return Err(BuiltinError::new(TrapReason::HashExpectsOneOperand));
            };
            MapKey::from_value(gc, value)
                .map(|key| Value::I64(key.script_hash()))
                .ok_or_else(|| {
                    BuiltinError::new(TrapReason::ValueHasNoHashSemanticsOrExceedsKeySizeLimit)
                })
        }
        RuntimePrimitive::ValueDebug | RuntimePrimitive::ValueDisplay => {
            let [value] = args else {
                return Err(BuiltinError::new(TrapReason::FormatExpectsOneOperand));
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
            TrapReason::ArrayJoinExpectsAStringArrayAndSeparator,
        ));
    };
    if !gc
        .array_contract(*handle)
        .is_some_and(|contract| contract.ty == Ty::Builtin(BuiltinType::String))
    {
        return Err(BuiltinError::new(
            TrapReason::StringInterpolationRequiresStringStorage,
        ));
    }
    let output = gc
        .with_buffer(*handle, |values| {
            let SequenceStorage::Traced(values) = values else {
                return Err(BuiltinError::new(
                    TrapReason::ArrayJoinExpectsStringElements,
                ));
            };
            let overflow =
                || BuiltinError::from(RuntimeError::resource_limit("joined string size"));
            let separator = gc
                .string(*separator)
                .ok_or_else(|| BuiltinError::new(TrapReason::InvalidStringSeparator))?;
            let mut length = separator
                .len()
                .checked_mul(values.len().saturating_sub(1))
                .ok_or_else(overflow)?;
            for value in values {
                let Value::Str(value) = value else {
                    return Err(BuiltinError::new(
                        TrapReason::ArrayJoinExpectsStringElements,
                    ));
                };
                let value = gc
                    .string(*value)
                    .ok_or_else(|| BuiltinError::new(TrapReason::InvalidStringElement))?;
                length = length.checked_add(value.len()).ok_or_else(overflow)?;
            }
            let mut output = String::new();
            output.try_reserve_exact(length).map_err(|_| overflow())?;
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push_str(&separator);
                }
                let Value::Str(value) = value else {
                    unreachable!("validated string element");
                };
                let value = gc
                    .string(*value)
                    .ok_or_else(|| BuiltinError::new(TrapReason::InvalidStringElement))?;
                output.push_str(&value);
            }
            Ok(output)
        })
        .ok_or_else(|| BuiltinError::new(TrapReason::ArrayJoinExpectsAValidArrayHandle))??;
    gc.alloc_string(output).map_err(Into::into)
}

fn debug_assert(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Bool(condition), Value::Str(message)] = args else {
        return Err(BuiltinError::new(
            TrapReason::DebugAssertExpectsBoolAndStringMessage,
        ));
    };
    if *condition {
        Ok(Value::Unit)
    } else {
        let message = gc
            .string(*message)
            .ok_or_else(|| BuiltinError::new(TrapReason::InvalidAssertionMessage))?;
        Err(BuiltinError::new(TrapReason::DebugAssertion {
            message: message.to_string(),
        }))
    }
}
