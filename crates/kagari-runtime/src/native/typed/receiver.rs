//! The physical receiver slot is separate from the Rust argument tuple.
use crate::{
    error::RuntimeError,
    frame::{call_slots::ArgumentReader, types::arguments::TypeArgument},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, KagariType,
            arguments::{FromKagariArguments, KagariArguments, PreparedArguments},
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
    fn read_prepared_arguments<'runtime>(
        cx: &mut ConversionContext<'runtime>,
        prepared: &PreparedArguments<'_, Self>,
        reader: &ArgumentReader<'_, 'runtime>,
    ) -> NativeResult<Self> {
        let expected = prepared.types();
        if reader.is_empty() {
            return Err(RuntimeError::module_validation(
                "typed receiver argument count",
            ));
        }
        reader.check_values(cx, expected)?;
        // The receiver converter may reenter before the rest of the pack is
        // decoded. Retain the whole input snapshot across that boundary.
        let receiver = reader.decode_prepared::<S>(cx, &expected[0], 0)?;
        let suffix = PreparedArguments::from_checked(&expected[1..]);
        let arguments = A::from_prepared_arguments(cx, &suffix, &reader.suffix(1)?)?;
        Ok(Self(receiver, arguments))
    }
}
