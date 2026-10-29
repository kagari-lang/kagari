//! The outer Rust tuple is an argument list; its elements each convert one value.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootSet,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        context::arguments::ArgumentView,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};

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
    fn into_arguments(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
    ) -> NativeResult<RootSet>;
}

/// Convert a protected argument pack, including the runtime's scoped loan adapters.
/// Ordinary custom adapters retain storable slots through ArgumentView instead of
/// copying raw values. A snapshot preserves the whole pack across child callbacks;
/// slicing that snapshot shares its existing root lease.
pub trait FromKagariArguments: KagariArguments + Sized {
    /// Check the whole signature before user-defined child conversion runs.
    /// Scoped capabilities retain their original loan validity and access checks.
    fn from_arguments(
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
        values: &ArgumentView<'_>,
    ) -> NativeResult<Self>;
}

/// Own the argument roots until the backend result has been retained or decoded.
pub(crate) fn encode_arguments<A: IntoKagariArguments>(
    cx: &mut ConversionContext<'_>,
    expected: &[TypeArgument],
    arguments: A,
) -> NativeResult<(RootSet, Vec<Value>)> {
    let roots = arguments
        .into_arguments(cx, expected)
        .map_err(|error| cx.runtime().resources().admit_execution_error(error))?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(expected.len())
        .map_err(|_| cx.runtime().resources().limit("typed call arguments"))?;
    for slot in 0..expected.len() {
        values.push(
            roots
                .get(cx.runtime().gc(), slot)
                .ok_or_else(|| RuntimeError::module_validation("typed call argument root"))?,
        );
    }
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

pub(crate) fn check_types<A: KagariArguments>(
    cx: &ConversionContext<'_>,
    expected: &[TypeArgument],
) -> NativeResult<()> {
    A::check_types(cx, expected)
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
            fn into_arguments(self, cx: &mut ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<RootSet> {
                check_arity(expected, $count)?;
                $(cx.check_type::<$ty>(&expected[$slot])?;)*
                cx.check_elements($count)?;
                cx.argument_scope(|cx| {
                    let values = vec![$(cx.encode_transport(&expected[$slot], self.$slot)?),*];
                    cx.runtime().gc().root_execution_values(values)
                        .ok_or_else(|| RuntimeError::module_validation("typed argument roots"))
                })
            }
        }

        impl<$($ty: FromKagari),*> FromKagariArguments for ($($ty,)*) {
            fn from_arguments(cx: &mut ConversionContext<'_>, expected: &[TypeArgument], values: &ArgumentView<'_>) -> NativeResult<Self> {
                check_arity(expected, $count)?;
                if values.len() != $count { return Err(RuntimeError::module_validation("typed input argument count")); }
                values.check_heap(cx.runtime().gc())?;
                let values = values.snapshot()?;
                $(cx.check_type::<$ty>(&expected[$slot])?;)*
                cx.check_elements($count)?;
                for (slot, ty) in expected.iter().enumerate() {
                    let value = values.get(slot).ok_or_else(|| RuntimeError::module_validation("typed input argument slot"))?;
                    cx.check_value(ty, &value)?;
                }
                let result = cx.argument_scope(|_cx| {
                    Ok(($({
                        let value = values.get($slot)
                            .ok_or_else(|| RuntimeError::module_validation("typed input argument slot"))?;
                        // SAFETY: The private argument snapshot retains this slot through admission.
                        unsafe { _cx.decode_transport::<$ty>(&expected[$slot], &value) }?
                    },)*))
                });
                drop(values);
                result
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
