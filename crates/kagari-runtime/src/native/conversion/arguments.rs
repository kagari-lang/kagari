//! The outer Rust tuple is an argument list; its elements each convert one value.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootSet,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};

pub trait KagariArguments {
    fn argument_types(catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>>;
}

pub trait IntoKagariArguments: KagariArguments + Sized {
    /// Checks the entire declared pack before converting its first argument.
    fn into_arguments(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
    ) -> NativeResult<RootSet>;
}

pub trait FromKagariArguments: KagariArguments + Sized {
    /// Retains a snapshot of the entire pack before user-defined conversion runs.
    fn from_arguments(
        cx: &mut ConversionContext<'_>,
        expected: &[TypeArgument],
        values: &[Value],
    ) -> NativeResult<Self>;
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
    let types = A::argument_types(&cx.runtime().native_entries.catalog)?;
    check_arity(expected, types.len())?;
    for (expected, ty) in expected.iter().zip(types) {
        cx.check_declared_type(expected, ty)?;
    }
    Ok(())
}

macro_rules! arguments {
    ($count:expr; $($ty:ident:$slot:tt),*) => {
        impl<$($ty: KagariType),*> KagariArguments for ($($ty,)*) {
            fn argument_types(_catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
                Ok(vec![$($ty::kagari_type(_catalog)?),*])
            }
        }

        impl<$($ty: IntoKagari),*> IntoKagariArguments for ($($ty,)*) {
            fn into_arguments(self, cx: &mut ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<RootSet> {
                check_arity(expected, $count)?;
                $(cx.check_type::<$ty>(&expected[$slot])?;)*
                cx.check_elements($count)?;
                cx.argument_scope(|cx| {
                    let values = vec![$(cx.encode_value(&expected[$slot], self.$slot)?),*];
                    cx.runtime().gc().root_execution_values(values)
                        .ok_or_else(|| RuntimeError::module_validation("typed argument roots"))
                })
            }
        }

        impl<$($ty: FromKagari),*> FromKagariArguments for ($($ty,)*) {
            fn from_arguments(cx: &mut ConversionContext<'_>, expected: &[TypeArgument], values: &[Value]) -> NativeResult<Self> {
                check_arity(expected, $count)?;
                if values.len() != $count { return Err(RuntimeError::module_validation("typed input argument count")); }
                $(cx.check_type::<$ty>(&expected[$slot])?;)*
                cx.check_elements($count)?;
                for (ty, value) in expected.iter().zip(values) { cx.check_value(ty, value)?; }
                let roots = cx.runtime().gc().root_execution_values(values.to_vec())
                    .ok_or_else(|| RuntimeError::module_validation("typed input argument roots"))?;
                let result = cx.argument_scope(|_cx| {
                    Ok(($({
                        let value = roots.get(_cx.runtime().gc(), $slot)
                            .ok_or_else(|| RuntimeError::module_validation("typed input argument slot"))?;
                        _cx.decode_value::<$ty>(&expected[$slot], &value)?
                    },)*))
                });
                drop(roots);
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
