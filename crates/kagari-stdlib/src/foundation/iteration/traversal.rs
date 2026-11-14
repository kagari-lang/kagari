//! Ordered construction traversal through explicit checked iterator calls.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols};
use kagari_runtime::{
    native::{binding::NativeResult, context::NativeCall},
    value::view::ValueView,
};
use std::slice;

pub(crate) fn for_each<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    first: usize,
    mut visit: impl for<'step> FnMut(&mut NativeCall<'step>, ValueView<'step>) -> NativeResult<()>,
) -> NativeResult<()> {
    let source = cx.argument(0)?;

    let iter = cx.selected_at(first)?;
    let next = cx.selected_at(first + 1)?;
    let cursor = cx.call_values(iter, &[source])?;
    loop {
        let done = cx.with_temporaries(|cx| {
            let value = cx.call_values(next, slice::from_ref(&cursor))?;
            let Some(item) = enums::read_option(symbols, cx, &value)? else {
                return Ok(true);
            };
            visit(cx, item)?;
            Ok(false)
        })?;
        if done {
            return Ok(());
        }
    }
}
