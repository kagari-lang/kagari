//! The physical receiver slot is separate from the Rust argument tuple.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, KagariType,
            arguments::{FromKagariArguments, KagariArguments, check_types},
            context::ConversionContext,
        },
        types::Type,
    },
    value::Value,
};

pub(super) struct ReceiverArguments<S, A>(pub S, pub A);

impl<S: KagariType, A: KagariArguments> KagariArguments for ReceiverArguments<S, A> {
    fn argument_types(catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
        let mut types = A::argument_types(catalog)?;
        types.insert(0, S::kagari_type(catalog)?);
        Ok(types)
    }
}

impl<S: FromKagari, A: FromKagariArguments> FromKagariArguments for ReceiverArguments<S, A> {
    fn from_arguments(
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
        values: &[Value],
    ) -> NativeResult<Self> {
        check_types::<Self>(cx, expected)?;
        if values.len() != expected.len() || values.is_empty() {
            return Err(RuntimeError::module_validation(
                "typed receiver argument count",
            ));
        }
        for (ty, value) in expected.iter().zip(values) {
            cx.check_value(ty, value)?;
        }
        // The receiver converter may reenter before the rest of the pack is
        // decoded. Retain the whole input snapshot across that boundary.
        let roots = cx
            .runtime()
            .gc()
            .root_execution_values(values.to_vec())
            .ok_or_else(|| RuntimeError::module_validation("typed receiver argument roots"))?;
        let receiver = cx.decode_value::<S>(&expected[0], &values[0])?;
        let arguments = A::from_arguments(cx, &expected[1..], &values[1..])?;
        drop(roots);
        Ok(Self(receiver, arguments))
    }
}
