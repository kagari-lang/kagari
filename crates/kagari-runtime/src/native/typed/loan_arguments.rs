//! Borrowed receivers decode capabilities; Rust references are created only later.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    host::scoped::ScopedHostView,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            arguments::{FromKagariArguments, KagariArguments},
            context::ConversionContext,
        },
        payload::Opaque,
        storage::NativePayload,
        types::Type,
    },
    value::Value,
};
use kagari_types::reference::ReferenceAccess;

pub(super) enum LoanReceiver<T> {
    Opaque(Opaque<T>),
    External(ScopedHostView),
}

pub(super) struct LoanArguments<T, A, const MUTABLE: bool>(pub LoanReceiver<T>, pub A);

impl<T: NativePayload, A: KagariArguments, const MUTABLE: bool> KagariArguments
    for LoanArguments<T, A, MUTABLE>
{
    fn argument_types(_: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
        Err(RuntimeError::module_validation(
            "borrowed receiver requires its explicit declaration",
        ))
    }

    fn check_types(cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
        let (receiver, arguments) = expected
            .split_first()
            .ok_or_else(|| RuntimeError::module_validation("missing borrowed receiver"))?;
        cx.check_type::<Opaque<T>>(receiver)?;
        A::check_types(cx, arguments)
    }
}

impl<T: NativePayload, A: FromKagariArguments, const MUTABLE: bool> FromKagariArguments
    for LoanArguments<T, A, MUTABLE>
{
    fn from_arguments(
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
        values: &[Value],
    ) -> NativeResult<Self> {
        Self::check_types(cx, expected)?;
        if values.len() != expected.len() || values.is_empty() {
            return Err(RuntimeError::module_validation(
                "borrowed receiver argument count",
            ));
        }
        let receiver = if matches!(values[0], Value::Ephemeral(_)) {
            let view = cx.runtime().scoped_view(&values[0])?;
            let access = if MUTABLE {
                ReferenceAccess::Mutable
            } else {
                ReferenceAccess::Shared
            };
            if !cx.runtime().host_borrows.scoped_matches(
                cx.runtime(),
                view,
                &expected[0],
                access,
                cx.owner(),
            ) {
                return Err(RuntimeError::module_validation(
                    "borrowed receiver target or capability mismatch",
                ));
            }
            LoanReceiver::External(view)
        } else {
            LoanReceiver::Opaque(cx.decode_value::<Opaque<T>>(&expected[0], &values[0])?)
        };
        // The ordinary invocation owner roots the entire input snapshot. All
        // owned conversions finish before either storage adapter acquires a loan.
        let arguments = A::from_arguments(cx, &expected[1..], &values[1..])?;
        Ok(Self(receiver, arguments))
    }
}
