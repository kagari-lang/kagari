//! The physical receiver slot is separate from the Rust argument tuple.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, KagariType,
            arguments::{ArgumentValues, FromKagariArguments, KagariArguments, PreparedArguments},
            context::ConversionContext,
        },
        types::Type,
    },
};

pub(super) struct ReceiverArguments<S, A>(pub S, pub A);

impl<S: KagariType, A: KagariArguments> KagariArguments for ReceiverArguments<S, A> {
    fn argument_types(catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
        let mut types = A::argument_types(catalog)?;
        types.insert(0, S::kagari_type(catalog)?);
        Ok(types)
    }

    fn check_types(cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
        let (receiver, arguments) = expected
            .split_first()
            .ok_or_else(|| RuntimeError::module_validation("missing receiver type"))?;
        cx.check_type::<S>(receiver)?;
        A::check_types(cx, arguments)
    }
}

impl<S: FromKagari, A: FromKagariArguments> FromKagariArguments for ReceiverArguments<S, A> {
    fn decode_arguments<'call>(
        cx: &mut ConversionContext<'call>,
        prepared: &PreparedArguments<'_, Self>,
        values: ArgumentValues<'call>,
    ) -> NativeResult<Self> {
        let expected = prepared.types();
        if values.len() != expected.len() || values.is_empty() {
            return Err(RuntimeError::module_validation(
                "typed receiver argument count",
            ));
        }
        // The outer binding admitted the whole immutable prefix. A receiver
        // converter may reenter without another copy or auxiliary root owner.
        let value = values.get(cx, 0)?;
        let receiver = cx.decode_admitted::<S>(&expected[0], &value)?;
        let arguments = PreparedArguments::<A>::from_binding(cx, &expected[1..])
            .decode_tail(cx, values.tail(1)?)?;
        Ok(Self(receiver, arguments))
    }
}
