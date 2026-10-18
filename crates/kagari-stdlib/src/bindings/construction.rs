//! Synchronous base conversions, parsing and iterable construction.
use crate::{bindings::enums, identity};
use kagari_bytecode::instruction::BinaryOp;
use kagari_common::identity::DefinitionKind;
use kagari_contract::numeric::NumericOperation;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::HeapObjectId,
    native::{binding::NativeResult, context::CallContext, scalar::NativeScalar},
    numeric,
    value::Value,
};
use kagari_types::{integer::IntegerOp, scalar::BuiltinType, ty::Ty};
use std::{
    num::{IntErrorKind, ParseIntError},
    slice,
};

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

fn result_item(cx: &CallContext<'_>) -> NativeResult<BuiltinType> {
    let Ty::Enum(nominal) = cx.result_type() else {
        return Err(invalid());
    };
    let args = &nominal.arguments;
    let Some(Ty::Builtin(target)) = args.first() else {
        return Err(invalid());
    };
    Ok(*target)
}

pub(super) fn from_str(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let Value::Str(text) = cx.argument(0)? else {
        return Err(invalid());
    };
    let parsed = {
        let text = cx.heap().string(text).ok_or_else(invalid)?;
        parse(result_item(cx)?, &text)?
    };
    result(
        cx,
        parsed.map_err(|error| {
            [
                "Empty",
                "InvalidDigit",
                "OutOfRange",
                "InvalidRadix",
                "InvalidSyntax",
            ][error as usize]
        }),
        "ParseError",
    )
}

fn result(
    cx: &CallContext<'_>,
    value: Result<Value, &str>,
    error_type: &str,
) -> NativeResult<Value> {
    let (member, value) = match value {
        Ok(value) => ("Ok", value),
        Err(member) => (
            "Err",
            enums::allocate(
                cx,
                &cx.result_type_parameter(1)?,
                error_type,
                member,
                vec![],
            )?,
        ),
    };
    let _root = cx.heap().root_value(value).ok_or_else(invalid)?;
    enums::allocate(
        cx,
        &cx.result_type_argument()?,
        "Result",
        member,
        vec![value],
    )
}

pub(super) fn try_from(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let Ty::Builtin(source) = cx.argument_type(0)? else {
        return Err(invalid());
    };
    let converted = numeric::checked_convert(*source, result_item(cx)?, cx.argument(0)?)?;
    result(cx, converted.ok_or("OutOfRange"), "TryFromIntError")
}

fn integer_error(error: ParseIntError) -> u8 {
    match error.kind() {
        IntErrorKind::Empty => 0,
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => 2,
        _ => 1,
    }
}

fn parse(target: BuiltinType, text: &str) -> NativeResult<Result<Value, u8>> {
    macro_rules! integers {
        ($($kind:ident: $rust:ty),+) => {
            match target {
                $(BuiltinType::$kind => Ok(text.parse::<$rust>().map(NativeScalar::encode).map_err(integer_error)),)+
                BuiltinType::F32 => Ok(text.parse::<f32>().map(NativeScalar::encode).map_err(|_| if text.is_empty() {0} else {4})),
                BuiltinType::F64 => Ok(text.parse::<f64>().map(NativeScalar::encode).map_err(|_| if text.is_empty() {0} else {4})),
                BuiltinType::Bool => Ok(text.parse::<bool>().map(NativeScalar::encode).map_err(|_| if text.is_empty() {0} else {4})),
                _ => Err(invalid()),
            }
        };
    }

    integers!(I8:i8,I16:i16,I32:i32,I64:i64,U8:u8,U16:u16,U32:u32,U64:u64)
}

pub(super) fn list_from_iter(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let Ty::NativeObject(_) = cx.result_type() else {
        return Err(invalid());
    };
    let result = cx.allocate_result()?;
    let _root = cx.heap().root_value(result).ok_or_else(invalid)?;
    let Value::GcHandle(id) = result else {
        return Err(invalid());
    };
    for_each(cx, 0, |cx, value| cx.heap().sequence_push(id, value))?;
    Ok(result)
}

pub(super) fn sum(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    aggregate(cx, false)
}

pub(super) fn product(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    aggregate(cx, true)
}

fn aggregate(cx: &mut CallContext<'_>, product: bool) -> NativeResult<Value> {
    let Ty::Builtin(kind) = cx.result_type() else {
        return Err(invalid());
    };
    let kind = *kind;
    if vec_argument(cx)?.is_some() {
        return scalar_vec_aggregate(cx, kind, product);
    }
    let identity = i32::from(product);
    let mut result = match kind {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => Value::I32(identity),
        BuiltinType::U64 => Value::U64(identity as u64),
        BuiltinType::F32 => Value::F32(identity as f32),
        BuiltinType::F64 => Value::F64(identity as f64),
        _ => Value::I64(i64::from(identity)),
    };
    for_each(cx, 0, |_, item| {
        result = if kind.integer_layout().is_some() {
            numeric::fixed_integer(
                NumericOperation {
                    op: if product {
                        IntegerOp::CheckedMul
                    } else {
                        IntegerOp::CheckedAdd
                    },
                    input: kind,
                    rhs: Some(kind),
                },
                result,
                Some(item),
            )?
        } else {
            numeric::binary(
                if product {
                    BinaryOp::Mul
                } else {
                    BinaryOp::Add
                },
                result,
                item,
            )?
        };
        Ok(())
    })?;
    Ok(result)
}

// Only the standard Vec implementation promises callback-free buffer traversal.
// Other native objects, even those with sequence storage, use their selected iterators.
fn vec_argument(cx: &CallContext<'_>) -> NativeResult<Option<HeapObjectId>> {
    let argument = cx.argument_type_argument(0)?;
    let Ty::NativeObject(nominal) = argument.ty() else {
        return Ok(None);
    };
    let declaration = identity::type_id("Vec", DefinitionKind::AssociatedType);
    if argument.definitions().lookup(&declaration) != Some(nominal.declaration) {
        return Ok(None);
    }
    let Value::GcHandle(id) = cx.argument(0)? else {
        return Err(invalid());
    };
    Ok(Some(id))
}

fn for_each(
    cx: &mut CallContext<'_>,
    first: usize,
    mut visit: impl FnMut(&mut CallContext<'_>, Value) -> NativeResult<()>,
) -> NativeResult<()> {
    let source = cx.argument(0)?;
    // The standard Vec has no iteration callbacks. Read its contiguous storage
    // directly without allocating a cursor or Option for each element.
    if let Some(id) = vec_argument(cx)? {
        let revision = cx.heap().collection_revision(&source).ok_or_else(invalid)?;
        let length = cx.heap().sequence_len(id).ok_or_else(invalid)?;
        for index in 0..length {
            if index % 256 == 0 {
                cx.poll()?;
            }
            cx.heap().check_collection_revision(&source, revision)?;
            visit(cx, cx.heap().sequence_get(id, index).ok_or_else(invalid)?)?;
        }
        return cx.heap().check_collection_revision(&source, revision);
    }
    let iter = cx.selected_at(first)?;
    let next = cx.selected_at(first + 1)?;
    let cursor = cx.call_values(iter, &[source])?;
    let _root = cx.heap().root_value(cursor).ok_or_else(invalid)?;
    loop {
        cx.poll()?;
        let value = cx.call_values(next, slice::from_ref(&cursor))?;
        let Some(item) = enums::option(cx, &value)? else {
            return Ok(());
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        visit(cx, item)?;
    }
}

// Match the physical element layout once, then reduce borrowed Rust scalars.
// The scoped borrow prevents structural mutation; this path invokes no callbacks.
fn scalar_vec_aggregate(
    cx: &CallContext<'_>,
    kind: BuiltinType,
    product: bool,
) -> NativeResult<Value> {
    macro_rules! integers {
        ($($kind:ident:$rust:ty),+) => {
            match kind {
                $(BuiltinType::$kind => cx.with_sequence::<$rust, _>(0, |items| {
                    let mut result: $rust = if product {1} else {0};
                    for chunk in items.chunks(256) {
                        cx.poll()?;
                        for item in chunk {
                            result = if product {result.checked_mul(*item)} else {result.checked_add(*item)}
                                .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "integer overflow"))?;
                        }
                    }
                    Ok(result.encode())
                }),)+
                BuiltinType::F32 => cx.with_sequence::<f32, _>(0, |items| {
                    let mut result = if product {1.0} else {0.0};
                    for chunk in items.chunks(256) { cx.poll()?; for item in chunk { result = if product {result * item} else {result + item}; } }
                    Ok(Value::F32(result))
                }),
                BuiltinType::F64 => cx.with_sequence::<f64, _>(0, |items| {
                    let mut result = if product {1.0} else {0.0};
                    for chunk in items.chunks(256) { cx.poll()?; for item in chunk { result = if product {result * item} else {result + item}; } }
                    Ok(Value::F64(result))
                }),
                _ => Err(invalid()),
            }
        };
    }

    integers!(I8:i8,I16:i16,I32:i32,I64:i64,U8:u8,U16:u16,U32:u32,U64:u64)
}

pub(super) fn hash_from_iter(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = cx.allocate_result()?;
    let _root = cx.heap().root_value(result).ok_or_else(invalid)?;
    let insert = cx.selected_at(2)?;
    let map = matches!(result, Value::Map(_));
    for_each(cx, 3, |cx, item| {
        if map {
            let Value::Tuple(id) = item else {
                return Err(invalid());
            };
            let (key, value) = {
                let tuple = cx.heap().tuple(id).ok_or_else(invalid)?;
                let [key, value] = &*tuple else {
                    return Err(invalid());
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

#[cfg(test)]
mod tests {
    use super::parse;
    use kagari_runtime::value::Value;
    use kagari_types::scalar::BuiltinType;

    #[test]
    fn parsing_preserves_fixed_width_integer_boundaries() {
        for (kind, text, expected) in [
            (BuiltinType::U64, "4294967296", Value::U64(1u64 << 32)),
            (
                BuiltinType::U64,
                "18446744073709551615",
                Value::U64(u64::MAX),
            ),
            (
                BuiltinType::I64,
                "-9223372036854775808",
                Value::I64(i64::MIN),
            ),
            (
                BuiltinType::I64,
                "9223372036854775807",
                Value::I64(i64::MAX),
            ),
        ] {
            assert_eq!(parse(kind, text).unwrap(), Ok(expected));
        }
        for (kind, text) in [
            (BuiltinType::U64, "18446744073709551616"),
            (BuiltinType::I64, "-9223372036854775809"),
            (BuiltinType::I64, "9223372036854775808"),
        ] {
            assert_eq!(parse(kind, text).unwrap(), Err(2));
        }
    }
}
