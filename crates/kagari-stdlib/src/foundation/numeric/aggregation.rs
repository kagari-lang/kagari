//! Numeric reductions retain the callback-free Vec path and checked arithmetic.
use crate::collections::vector::scalar::with_argument_slice;
use crate::foundation::{
    enumerations::value::EnumSymbols,
    iteration::traversal::{for_each, vec_argument},
};
use kagari_bytecode::instruction::BinaryOp;
use kagari_contract::numeric::NumericOperation;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{binding::NativeResult, context::CallContext, scalar::NativeScalar},
    numeric,
    value::Value,
};
use kagari_types::{integer::IntegerOp, scalar::BuiltinType, ty::Ty};

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

pub(crate) fn sum(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    aggregate(symbols, cx, false)
}

pub(crate) fn product(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    aggregate(symbols, cx, true)
}

fn aggregate(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    product: bool,
) -> NativeResult<Value> {
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
    for_each(symbols, cx, 0, |_, item| {
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
                $(BuiltinType::$kind => with_argument_slice::<$rust, _>(cx, 0, |items| {
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
                BuiltinType::F32 => with_argument_slice::<f32, _>(cx, 0, |items| {
                    let mut result = if product {1.0} else {0.0};
                    for chunk in items.chunks(256) { cx.poll()?; for item in chunk { result = if product {result * item} else {result + item}; } }
                    Ok(Value::F32(result))
                }),
                BuiltinType::F64 => with_argument_slice::<f64, _>(cx, 0, |items| {
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
