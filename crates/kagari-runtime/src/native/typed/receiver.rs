//! The physical receiver slot is separate from the Rust argument tuple.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        context::arguments::ArgumentView,
        conversion::{
            FromKagari, KagariType,
            arguments::{FromKagariArguments, KagariArguments, check_types},
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
    fn from_arguments(
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
        values: &ArgumentView<'_>,
    ) -> NativeResult<Self> {
        values.check_heap(cx.runtime().gc())?;
        let values = values.snapshot()?;
        check_types::<Self>(cx, expected)?;
        if values.len() != expected.len() || values.is_empty() {
            return Err(RuntimeError::module_validation(
                "typed receiver argument count",
            ));
        }
        for (slot, ty) in expected.iter().enumerate() {
            let value = values
                .get(slot)
                .ok_or_else(|| RuntimeError::module_validation("typed receiver argument slot"))?;
            cx.check_value(ty, &value)?;
        }
        let value = values
            .get(0)
            .ok_or_else(|| RuntimeError::module_validation("typed receiver argument slot"))?;
        // SAFETY: The immutable complete argument snapshot retains the receiver.
        let receiver = unsafe { cx.decode_transport::<S>(&expected[0], &value) }?;
        let tail = values
            .slice(1..values.len())
            .ok_or_else(|| RuntimeError::module_validation("typed receiver argument range"))?;
        let arguments = A::from_arguments(cx, &expected[1..], &tail)?;
        Ok(Self(receiver, arguments))
    }
}
