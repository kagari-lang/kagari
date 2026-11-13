//! Borrowed receivers decode capabilities; Rust references are created only later.
use crate::{
    error::RuntimeError,
    execution::roots::ValueLocation,
    frame::types::arguments::TypeArgument,
    host::scoped::ScopedHostView,
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

pub(super) enum LoanReceiver<T> {
    Native(NativeReceiver<T>),
    External(ScopedHostView),
}

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
    value: &ValueView<'_>,
) -> NativeResult<LoanReceiver<T>> {
    if value.kind() == Some(ObjectKind::Ephemeral) {
        let target = target(cx, expected)?;
        let view = cx.runtime_mut().scoped_view(value)?;
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
    } else if matches!(expected.ty(), Ty::NativeObject(_))
        && value.kind() == Some(ObjectKind::Native)
    {
        // Fixed Rust mapping and complete value admission belong to the binding.
        // Keep only a source coordinate until all owned converters have finished.
        Ok(LoanReceiver::Native(NativeReceiver {
            value: value
                .location()
                .ok_or_else(|| RuntimeError::module_validation("Native receiver source"))?,
            applied: expected.clone(),
            marker: PhantomData,
        }))
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
        let receiver = receiver::<T, MUTABLE>(cx, &expected[0], &value)?;
        // The invocation owns the immutable prefix. Finish owned conversions
        // before either storage adapter acquires a Rust payload loan.
        let arguments = PreparedArguments::<A>::from_binding(cx, &expected[1..])
            .decode_tail(cx, values.tail(1)?)?;
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
    fn decode_arguments<'call>(
        cx: &mut ConversionContext<'call>,
        prepared: &PreparedArguments<'_, Self>,
        values: ArgumentValues<'call>,
    ) -> NativeResult<Self> {
        let expected = prepared.types();
        if values.len() != expected.len() || values.len() < 2 {
            return Err(RuntimeError::module_validation(
                "borrowed pair argument count",
            ));
        }
        let value = values.get(cx, 0)?;
        let first = receiver::<T, M1>(cx, &expected[0], &value)?;
        let value = values.get(cx, 1)?;
        let second = receiver::<U, M2>(cx, &expected[1], &value)?;
        let owned = PreparedArguments::<A>::from_binding(cx, &expected[2..])
            .decode_tail(cx, values.tail(2)?)?;
        Ok(Self(first, second, owned))
    }
}
