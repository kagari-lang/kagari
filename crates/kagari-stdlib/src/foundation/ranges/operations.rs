//! Range bounds use the Native activation's prepared lexical signature and source.
use kagari_runtime::{
    native::{binding::NativeResult, context::NativeCall},
    value::view::ValueView,
};

pub(crate) fn start_bound<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    range_bound(cx, false)
}

pub(crate) fn end_bound<'call>(cx: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    range_bound(cx, true)
}

fn range_bound<'call>(cx: &mut NativeCall<'call>, upper: bool) -> NativeResult<ValueView<'call>> {
    let value = cx.argument(0)?;
    let range = cx.argument_type_argument(0)?;
    let bound = cx.result_type_argument()?;
    cx.range_bound(&value, &range, &bound, upper)
}
