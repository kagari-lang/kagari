//! Binding/application preparation owns stable conversion and scoped slot facts.
use crate::{
    frame::types::arguments::ScopedSignature,
    native::{
        binding::NativeResult,
        conversion::{IntoKagari, arguments::FromKagariArguments, context::ConversionContext},
    },
};

pub(super) fn prepare<A: FromKagariArguments, R: IntoKagari>(
    context: &ConversionContext<'_>,
    signature: &ScopedSignature,
) -> NativeResult<()> {
    context.check_type::<R>(&signature.result)?;
    A::check_types(context, &signature.params)
}
