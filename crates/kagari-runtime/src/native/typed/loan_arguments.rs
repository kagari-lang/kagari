//! Borrowed receivers decode capabilities; Rust references are created only later.
use crate::{
    error::RuntimeError,
    execution::roots::ValueLocation,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            arguments::{ArgumentValues, FromKagariArguments, KagariArguments, PreparedArguments},
            context::{ConversionContext, native::loans::NativeArgument},
        },
        rust_type::check_mapping,
        types::Type,
    },
    value::{kind::ObjectKind, view::ValueView},
};
use kagari_types::{reference::ReferenceAccess, ty::Ty};
use std::marker::PhantomData;

pub(super) struct NativeReceiver<T> {
    value: ValueLocation,
    applied: TypeArgument,
    marker: PhantomData<fn() -> T>,
}

impl<T> NativeReceiver<T> {
    pub(super) fn request(&self, access: ReferenceAccess) -> NativeArgument<'_> {
        NativeArgument {
            value: self.value,
            applied: &self.applied,
            access,
        }
    }
}

pub(super) struct LoanArguments<T, A>(pub NativeReceiver<T>, pub A);

fn receiver<T>(expected: &TypeArgument, value: &ValueView<'_>) -> NativeResult<NativeReceiver<T>> {
    if matches!(expected.ty(), Ty::NativeObject(_)) && value.kind() == Some(ObjectKind::Native) {
        // Fixed Rust mapping and complete value admission belong to the binding.
        // Keep only a source coordinate until all owned converters have finished.
        Ok(NativeReceiver {
            value: value
                .location()
                .ok_or_else(|| RuntimeError::module_validation("Native receiver source"))?,
            applied: expected.clone(),
            marker: PhantomData,
        })
    } else {
        Err(RuntimeError::module_validation(
            "borrowed receiver requires an Opaque payload",
        ))
    }
}

impl<T: Send + 'static, A: KagariArguments> KagariArguments for LoanArguments<T, A> {
    fn argument_types(_: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
        Err(RuntimeError::module_validation(
            "borrowed receiver requires its explicit declaration",
        ))
    }

    fn check_types(cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
        let (receiver, arguments) = expected
            .split_first()
            .ok_or_else(|| RuntimeError::module_validation("missing borrowed receiver"))?;
        check_mapping::<T>(cx.runtime(), cx.owner(), receiver)?;
        A::check_types(cx, arguments)
    }
}

impl<T: Send + 'static, A: FromKagariArguments> FromKagariArguments for LoanArguments<T, A> {
    fn decode_arguments<'call>(
        cx: &mut ConversionContext<'call>,
        prepared: &PreparedArguments<'_, Self>,
        values: ArgumentValues<'call>,
    ) -> NativeResult<Self> {
        let expected = prepared.types();
        if values.len() != expected.len() || values.is_empty() {
            return Err(RuntimeError::module_validation(
                "borrowed receiver argument count",
            ));
        }
        let value = values.get(cx, 0)?;
        let receiver = receiver::<T>(&expected[0], &value)?;
        // The invocation owns the immutable prefix. Finish owned conversions
        // before acquiring the Rust payload loan.
        let arguments = PreparedArguments::<A>::from_binding(cx, &expected[1..])
            .decode_tail(cx, values.tail(1)?)?;
        Ok(Self(receiver, arguments))
    }
}
