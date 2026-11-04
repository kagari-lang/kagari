//! The outer Rust tuple is an argument list; its elements each convert one value.
use crate::{
    error::RuntimeError,
    frame::{
        call_slots::{ArgumentReader, ArgumentWriter, CallSlots},
        types::arguments::TypeArgument,
    },
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};
use std::marker::PhantomData;

/// A checked Rust argument mapping for one immutable scoped signature.
///
/// Construction checks the complete pack before any converter runs. This proof
/// does not validate actual values or authorize access to another Runtime; the
/// encoder still checks values and protects them through allocating conversion.
pub struct PreparedArguments<'signature, A> {
    expected: &'signature [TypeArgument],
    mapping: PhantomData<fn(A)>,
}

impl<'signature, A: KagariArguments> PreparedArguments<'signature, A> {
    /// Check the whole Rust mapping, rejecting arity/type mismatch before effects.
    pub fn check(
        cx: &ConversionContext<'_>,
        expected: &'signature [TypeArgument],
    ) -> NativeResult<Self> {
        A::check_types(cx, expected)?;
        Ok(Self::from_checked(expected))
    }
}

impl<'signature, A> PreparedArguments<'signature, A> {
    /// Inspect the supplied script types when implementing a custom pack adapter.
    pub fn types(&self) -> &'signature [TypeArgument] {
        self.expected
    }

    /// The owning typed Function has already checked this A against its frozen
    /// declaration. Reload transports that contract into the selected signature.
    pub(in crate::native) fn from_checked(expected: &'signature [TypeArgument]) -> Self {
        Self {
            expected,
            mapping: PhantomData,
        }
    }
}

pub trait KagariArguments {
    fn argument_types(catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>>;

    fn check_types(cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
        let types = Self::argument_types(&cx.runtime().native_entries.catalog)?;
        check_arity(expected, types.len())?;
        for (expected, ty) in expected.iter().zip(types) {
            cx.check_declared_type(expected, ty)?;
        }
        Ok(())
    }
}

pub trait IntoKagariArguments: KagariArguments + Sized {
    /// Checks the entire declared pack before converting its first argument.
    fn into_arguments<'runtime>(
        self,
        cx: &mut ConversionContext<'runtime>,
        expected: &[TypeArgument],
    ) -> NativeResult<CallSlots<'runtime>> {
        let prepared = PreparedArguments::<Self>::check(cx, expected)?;
        self.into_prepared_arguments(cx, &prepared)
    }

    /// Encode with the supplied mapping proof, retaining actual-value checks.
    fn into_prepared_arguments<'runtime>(
        self,
        cx: &mut ConversionContext<'runtime>,
        prepared: &PreparedArguments<'_, Self>,
    ) -> NativeResult<CallSlots<'runtime>> {
        let mut slots = CallSlots::new(cx.runtime(), cx.owner(), prepared.types().len())?;
        self.write_prepared_arguments(cx, prepared, &mut slots.writer(prepared.types())?)?;
        slots.finish_arguments()?;
        Ok(slots)
    }

    /// Publish each converted argument through the bounded writer. No bank loan
    /// may cross user conversion, collection or reentry.
    fn write_prepared_arguments<'runtime>(
        self,
        cx: &mut ConversionContext<'runtime>,
        prepared: &PreparedArguments<'_, Self>,
        writer: &mut ArgumentWriter<'_, '_, 'runtime>,
    ) -> NativeResult<()>;
}

pub trait FromKagariArguments: KagariArguments + Sized {
    /// Retains a snapshot of the entire pack before user-defined conversion runs.
    fn from_arguments(
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
        values: &[Value],
    ) -> NativeResult<Self> {
        let prepared = PreparedArguments::<Self>::check(cx, expected)?;
        check_arity(expected, values.len())?;
        let slots = CallSlots::snapshot(cx.runtime(), cx.owner(), values.len(), |index| {
            Ok(values[index])
        })?;
        Self::from_prepared_arguments(cx, &prepared, &slots.reader())
    }

    /// Decode an already admitted Rust mapping from a protected snapshot.
    fn from_prepared_arguments<'runtime>(
        cx: &mut ConversionContext<'runtime>,
        prepared: &PreparedArguments<'_, Self>,
        reader: &ArgumentReader<'_, 'runtime>,
    ) -> NativeResult<Self> {
        check_arity(prepared.types(), reader.len())?;
        Self::read_prepared_arguments(cx, prepared, reader)
    }

    /// Implement the pack's conversion policy using bounded, protected reads.
    /// The supplied mapping covers this pack; actual value checks and conversion
    /// limits still belong to its read/decode operations.
    fn read_prepared_arguments<'runtime>(
        cx: &mut ConversionContext<'runtime>,
        prepared: &PreparedArguments<'_, Self>,
        reader: &ArgumentReader<'_, 'runtime>,
    ) -> NativeResult<Self>;
}

/// Own the argument roots until the backend result has been retained or decoded.
pub(crate) fn encode_arguments<'runtime, A: IntoKagariArguments>(
    cx: &mut ConversionContext<'runtime>,
    expected: &[TypeArgument],
    arguments: A,
) -> NativeResult<(CallSlots<'runtime>, Vec<Value>)> {
    let roots = arguments
        .into_arguments(cx, expected)
        .map_err(|error| cx.runtime().resources().admit_execution_error(error))?;
    let values = roots.values()?;
    Ok((roots, values))
}

fn check_arity(expected: &[TypeArgument], actual: usize) -> NativeResult<()> {
    if expected.len() != actual {
        return Err(RuntimeError::module_validation(
            "typed argument count differs from its declaration",
        ));
    }
    Ok(())
}

macro_rules! arguments {
    ($count:expr; $($ty:ident:$slot:tt),*) => {
        impl<$($ty: KagariType),*> KagariArguments for ($($ty,)*) {
            fn argument_types(_catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
                Ok(vec![$($ty::kagari_type(_catalog)?),*])
            }

            fn check_types(_cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
                check_arity(expected, $count)?;
                $(_cx.check_type::<$ty>(&expected[$slot])?;)*
                Ok(())
            }
        }

        impl<$($ty: IntoKagari),*> IntoKagariArguments for ($($ty,)*) {
            fn write_prepared_arguments<'runtime>(self, cx: &mut ConversionContext<'runtime>, prepared: &PreparedArguments<'_, Self>, _writer: &mut ArgumentWriter<'_, '_, 'runtime>) -> NativeResult<()> {
                let expected = prepared.types();
                debug_assert_eq!(expected.len(), $count);
                cx.check_elements($count)?;
                cx.argument_scope(|_cx| {
                    $(_writer.encode_prepared(_cx, $slot, self.$slot)?;)*
                    Ok(())
                })
            }
        }

        impl<$($ty: FromKagari),*> FromKagariArguments for ($($ty,)*) {
            fn read_prepared_arguments<'runtime>(cx: &mut ConversionContext<'runtime>, prepared: &PreparedArguments<'_, Self>, reader: &ArgumentReader<'_, 'runtime>) -> NativeResult<Self> {
                let expected = prepared.types();
                debug_assert_eq!(expected.len(), $count);
                cx.check_elements($count)?;
                reader.check_values(cx, expected)?;
                cx.argument_scope(|_cx| {
                    Ok(($(reader.decode_prepared::<$ty>(_cx, &expected[$slot], $slot)?,)*))
                })
            }
        }
    };
}

arguments!(0;);
arguments!(1; A:0);
arguments!(2; A:0, B:1);
arguments!(3; A:0, B:1, C:2);
arguments!(4; A:0, B:1, C:2, D:3);
arguments!(5; A:0, B:1, C:2, D:3, E:4);
arguments!(6; A:0, B:1, C:2, D:3, E:4, F:5);
arguments!(7; A:0, B:1, C:2, D:3, E:4, F:5, G:6);
arguments!(8; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7);
arguments!(9; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8);
arguments!(10; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9);
arguments!(11; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10);
arguments!(12; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11);
