//! Native range projection reuses its admitted lexical type signature and source.
use crate::{
    frame::types::arguments::TypeArgument,
    native::{binding::NativeResult, context::NativeCall},
    value::view::ValueView,
};

impl<'call> NativeCall<'call> {
    /// Project a bound from a protected range using the supplied declared type
    /// scopes. The canonical converter validates shape, integer domain and nominal
    /// member identity, then publishes into this Native call before cleanup.
    pub fn range_bound(
        &mut self,
        value: &ValueView<'_>,
        range: &TypeArgument,
        bound: &TypeArgument,
        upper: bool,
    ) -> NativeResult<ValueView<'call>> {
        self.convert_value(|conversion| conversion.range_bound(value, range, bound, upper))
    }
}
