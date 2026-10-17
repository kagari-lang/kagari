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
        rust_type::check_mapping,
        types::Type,
    },
    value::Value,
};
use kagari_types::reference::ReferenceAccess;
use kagari_types::ty::Ty;

pub(super) enum LoanReceiver<T> {
    Opaque(Opaque<T>),
    External(ScopedHostView),
}

pub(super) struct LoanArguments<T, A, const MUTABLE: bool>(pub LoanReceiver<T>, pub A);

pub(super) struct PairArguments<T, U, A, const FIRST_MUT: bool, const SECOND_MUT: bool>(
    pub LoanReceiver<T>,
    pub LoanReceiver<U>,
    pub A,
);

fn target(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<TypeArgument> {
    if matches!(expected.ty(), Ty::Reference { .. }) {
        cx.parameter(expected, 0)
    } else {
        Ok(expected.clone())
    }
}

fn receiver<T: Send + 'static, const MUTABLE: bool>(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: &Value,
) -> NativeResult<LoanReceiver<T>> {
    let target = target(cx, expected)?;
    check_mapping::<T>(cx.runtime(), cx.owner(), &target)?;
    if matches!(value, Value::Ephemeral(_)) {
        let view = cx.runtime().scoped_view(value)?;
        let access = if MUTABLE {
            ReferenceAccess::Mutable
        } else {
            ReferenceAccess::Shared
        };
        if !cx.runtime().host_borrows.scoped_matches(
            cx.runtime(),
            view,
            &target,
            access,
            cx.owner(),
        ) {
            return Err(RuntimeError::module_validation(
                "borrowed argument target or capability mismatch",
            ));
        }
        Ok(LoanReceiver::External(view))
    } else if !matches!(expected.ty(), Ty::Reference { .. }) {
        Ok(LoanReceiver::Opaque(Opaque::from_checked(
            cx, &target, value,
        )?))
    } else {
        Err(RuntimeError::module_validation(
            "reference parameter requires an external location",
        ))
    }
}

impl<T: Send + 'static, A: KagariArguments, const MUTABLE: bool> KagariArguments
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
        check_mapping::<T>(cx.runtime(), cx.owner(), &target(cx, receiver)?)?;
        A::check_types(cx, arguments)
    }
}

impl<T: Send + 'static, A: FromKagariArguments, const MUTABLE: bool> FromKagariArguments
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
        let receiver = receiver::<T, MUTABLE>(cx, &expected[0], &values[0])?;
        // The ordinary invocation owner roots the entire input snapshot. All
        // owned conversions finish before either storage adapter acquires a loan.
        let arguments = A::from_arguments(cx, &expected[1..], &values[1..])?;
        Ok(Self(receiver, arguments))
    }
}

impl<T: Send + 'static, U: Send + 'static, A: KagariArguments, const M1: bool, const M2: bool>
    KagariArguments for PairArguments<T, U, A, M1, M2>
{
    fn argument_types(_: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
        Err(RuntimeError::module_validation(
            "borrowed pack requires its explicit declaration",
        ))
    }

    fn check_types(cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
        if expected.len() < 2 {
            return Err(RuntimeError::module_validation("missing borrowed pair"));
        }
        check_mapping::<T>(cx.runtime(), cx.owner(), &target(cx, &expected[0])?)?;
        check_mapping::<U>(cx.runtime(), cx.owner(), &target(cx, &expected[1])?)?;
        A::check_types(cx, &expected[2..])
    }
}

impl<T: Send + 'static, U: Send + 'static, A: FromKagariArguments, const M1: bool, const M2: bool>
    FromKagariArguments for PairArguments<T, U, A, M1, M2>
{
    fn from_arguments(
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
        values: &[Value],
    ) -> NativeResult<Self> {
        Self::check_types(cx, expected)?;
        if values.len() != expected.len() {
            return Err(RuntimeError::module_validation(
                "borrowed pair argument count",
            ));
        }
        let first = receiver::<T, M1>(cx, &expected[0], &values[0])?;
        let second = receiver::<U, M2>(cx, &expected[1], &values[1])?;
        let owned = A::from_arguments(cx, &expected[2..], &values[2..])?;
        Ok(Self(first, second, owned))
    }
}
