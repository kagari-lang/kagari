//! Numeric reductions use checked iterator calls and source-width arithmetic.
use crate::foundation::{enumerations::value::EnumSymbols, iteration::traversal::for_each};
use kagari_bytecode::instruction::BinaryOp;
use kagari_contract::numeric::NumericOperation;
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::NativeCall},
    numeric,
    value::view::ValueView,
};
use kagari_types::{integer::IntegerOp, scalar::BuiltinType, ty::Ty};

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

pub(crate) fn sum<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    aggregate(symbols, cx, false)
}

pub(crate) fn product<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    aggregate(symbols, cx, true)
}

fn aggregate<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    product: bool,
) -> NativeResult<ValueView<'call>> {
    let Ty::Builtin(kind) = cx.result_type() else {
        return Err(invalid());
    };
    let kind = *kind;

    let identity = i32::from(product);
    let mut result = match kind {
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => ValueView::from(identity),
        BuiltinType::U64 => ValueView::from(identity as u64),
        BuiltinType::F32 => ValueView::from(identity as f32),
        BuiltinType::F64 => ValueView::from(identity as f64),
        _ => ValueView::from(i64::from(identity)),
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
