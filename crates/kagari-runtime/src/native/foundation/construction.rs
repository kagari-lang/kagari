//! Synchronous base conversions, parsing and iterable construction.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult, context::CallContext, declarations::SelectedCall,
        scalar::NativeScalar,
    },
    numeric,
    value::{EnumTag, Value},
};
use kagari_abi::{
    numeric::NumericOperation, scalar::BuiltinType, standard::surface::StandardEnum, types::AbiType,
};
use kagari_bytecode::instruction::BinaryOp;
use kagari_common::integer::IntegerOp;
use std::{
    num::{IntErrorKind, ParseIntError},
    slice,
};

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

fn result_item(cx: &CallContext<'_>) -> NativeResult<BuiltinType> {
    let AbiType::StandardEnum {
        kind: StandardEnum::Result,
        args,
    } = cx.result_type()
    else {
        return Err(invalid());
    };
    let Some(AbiType::Builtin(target)) = args.first() else {
        return Err(invalid());
    };
    Ok(*target)
}

pub(super) fn from_str(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let Value::Str(text) = cx.argument(0)? else {
        return Err(invalid());
    };
    let parsed = parse(result_item(cx)?, &text)?;
    let (tag, value) = match parsed {
        Ok(value) => (EnumTag::ResultOk, value),
        Err(error) => (
            EnumTag::ResultErr,
            Value::Enum(cx.heap().alloc_enum(EnumTag::ParseError(error), vec![])?),
        ),
    };
    let _root = cx.heap().root_value(value.clone()).ok_or_else(invalid)?;
    Ok(Value::Enum(cx.heap().alloc_enum(tag, vec![value])?))
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
    integers!(I8:i8,I16:i16,I32:i32,I64:i64,ISize:isize,U8:u8,U16:u16,U32:u32,U64:u64,USize:usize)
}

pub(super) fn list_from_iter(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let AbiType::Array(item, _) = cx.result_type() else {
        return Err(invalid());
    };
    let result = cx.allocate_sequence((**item).clone(), vec![])?;
    let _root = cx.heap().root_value(result.clone()).ok_or_else(invalid)?;
    let Value::Array(id) = result else {
        return Err(invalid());
    };
    for_each(cx, |cx, value| cx.heap().array_push(id, value))?;
    Ok(result)
}

pub(super) fn sum(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    aggregate(cx, false)
}
pub(super) fn product(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    aggregate(cx, true)
}

fn aggregate(cx: &mut CallContext<'_>, product: bool) -> NativeResult<Value> {
    let AbiType::Builtin(kind) = cx.result_type() else {
        return Err(invalid());
    };
    let kind = *kind;
    if matches!(cx.argument_type(0)?, AbiType::Array(_, _)) {
        return scalar_array_aggregate(cx, kind, product);
    }
    let identity = i32::from(product);
    let mut result = match kind {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => Value::I32(identity),
        BuiltinType::U64 | BuiltinType::USize => Value::U64(identity as u64),
        BuiltinType::F32 => Value::F32(identity as f32),
        BuiltinType::F64 => Value::F64(identity as f64),
        _ => Value::I64(i64::from(identity)),
    };
    for_each(cx, |_, item| {
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
                result.clone(),
                Some(item),
            )?
        } else {
            numeric::binary(
                if product {
                    BinaryOp::Mul
                } else {
                    BinaryOp::Add
                },
                result.clone(),
                item,
            )?
        };
        Ok(())
    })?;
    Ok(result)
}

fn for_each(
    cx: &mut CallContext<'_>,
    mut visit: impl FnMut(&mut CallContext<'_>, Value) -> NativeResult<()>,
) -> NativeResult<()> {
    let source = cx.argument(0)?;
    // A foundation array has no user callbacks. Read its contiguous storage
    // directly without allocating a cursor or Option for each element.
    if let Value::Array(id) = source {
        let length = cx.heap().array_len(id).ok_or_else(invalid)?;
        for index in 0..length {
            if index % 256 == 0 {
                cx.poll()?;
            }
            visit(cx, cx.heap().array_get(id, index).ok_or_else(invalid)?)?;
        }
        return Ok(());
    }
    let iter = cx.selected(SelectedCall { slot: 0 })?;
    let next = cx.selected(SelectedCall { slot: 1 })?;
    let cursor = cx.call_values(iter, &[source])?;
    let _root = cx.heap().root_value(cursor.clone()).ok_or_else(invalid)?;
    loop {
        cx.poll()?;
        let value = cx.call_values(next, slice::from_ref(&cursor))?;
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let option = cx.heap().enum_snapshot(id).ok_or_else(invalid)?;
        match option.tag {
            EnumTag::OptionNone => return Ok(()),
            EnumTag::OptionSome => {
                let [item] = option.fields.as_slice() else {
                    return Err(invalid());
                };
                // Retain a yielded reference through allocations in the visitor.
                let _item = cx.heap().root_value(item.clone()).ok_or_else(invalid)?;
                visit(cx, item.clone())?;
            }
            _ => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ModuleValidation,
                    "iterator returned a non-Option value",
                ));
            }
        }
    }
}

// Match the physical element layout once, then reduce borrowed Rust scalars.
// The scoped borrow prevents structural mutation; this path invokes no callbacks.
fn scalar_array_aggregate(
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
    integers!(I8:i8,I16:i16,I32:i32,I64:i64,ISize:isize,U8:u8,U16:u16,U32:u32,U64:u64,USize:usize)
}
