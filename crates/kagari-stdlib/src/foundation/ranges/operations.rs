//! Range bound projection through checked pinned enum layouts.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols};
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext},
    value::Value,
};
use std::ops::Bound;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid foundation receiver")
}

pub(crate) fn start_bound(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    range_bound(symbols, cx, false)
}

pub(crate) fn end_bound(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    range_bound(symbols, cx, true)
}

fn range_bound(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    upper: bool,
) -> NativeResult<Value> {
    // SAFETY: The call frame retains the range and its bounds through result publication.
    let Value::Range(range) = unsafe { cx.argument_transport(0) }? else {
        return Err(invalid());
    };
    let range = unsafe { cx.heap().range_transport(range) }.ok_or_else(invalid)?;
    let (member, fields) = match range.bound(
        cx.owner().definitions(),
        cx.argument_type(0)?,
        cx.result_type(),
        upper,
    )? {
        Bound::Included(value) => ("Included", vec![value]),
        Bound::Excluded(value) => ("Excluded", vec![value]),
        Bound::Unbounded => ("Unbounded", vec![]),
    };
    enums::allocate(
        symbols,
        cx,
        &cx.result_type_argument()?,
        "Bound",
        member,
        fields,
    )
}
