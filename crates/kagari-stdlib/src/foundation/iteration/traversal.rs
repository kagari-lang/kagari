//! Ordered construction traversal through explicit checked iterator calls.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols};
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext},
    value::Value,
};
use std::slice;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

pub(crate) fn for_each(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    first: usize,
    mut visit: impl FnMut(&mut CallContext<'_>, Value) -> NativeResult<()>,
) -> NativeResult<()> {
    let source = cx.argument(0)?;

    let iter = cx.selected_at(first)?;
    let next = cx.selected_at(first + 1)?;
    let cursor = cx.call_values(iter, &[source])?;
    let _root = cx.heap().root_value(cursor).ok_or_else(invalid)?;
    loop {
        cx.poll()?;
        let value = cx.call_values(next, slice::from_ref(&cursor))?;
        let Some(item) = enums::read_option(symbols, cx, &value)? else {
            return Ok(());
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        visit(cx, item)?;
    }
}
