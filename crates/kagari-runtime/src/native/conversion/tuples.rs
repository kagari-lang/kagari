use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        schema::TypeRegistration,
        types::Type,
    },
    value::view::ValueView,
};
use kagari_types::ty::Ty;

macro_rules! tuple {
    ($count:expr; $($ty:ident:$slot:tt),+) => {
        impl<$($ty: KagariType),+> KagariType for ($($ty,)+) {
            fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
            where Self: 'static {
                $(registration.dependency::<$ty>()?;)+
                Self::kagari_type(registration.catalog())
            }

            fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
                Type::tuple([$($ty::kagari_type(catalog)?),+])
            }

            fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
                if !matches!(expected.ty(), Ty::Tuple(items) if items.len() == $count) {
                    return Err(RuntimeError::module_validation("tuple conversion type"));
                }
                $(cx.check_type::<$ty>(&cx.parameter(expected, $slot)?)?;)+
                Ok(())
            }
        }

        impl<$($ty: IntoKagari),+> IntoKagari for ($($ty,)+) {
            fn into_kagari<'call>(self, cx: &mut ConversionContext<'call>, expected: &TypeArgument) -> NativeResult<ValueView<'call>> {
                cx.check_elements($count)?;
                let values = [$({
                    let element = cx.parameter(expected, $slot)?;
                    cx.encode_value(&element, self.$slot)?
                }),+];
                cx.allocate_tuple(&values)
            }
        }

        impl<$($ty: FromKagari),+> FromKagari for ($($ty,)+) {
            fn from_kagari(cx: &mut ConversionContext<'_>, expected: &TypeArgument, value: &ValueView<'_>) -> NativeResult<Self> {
                if cx.tuple_len(value)? != $count { return Err(RuntimeError::module_validation("tuple conversion arity")); }
                cx.check_elements($count)?;
                Ok(($({
                    let element = cx.parameter(expected, $slot)?;
                    let value = cx.tuple_element(value, $slot)?;
                    cx.decode_value::<$ty>(&element, &value)?
                },)+))
            }
        }
    };
}

tuple!(1; A:0);
tuple!(2; A:0, B:1);
tuple!(3; A:0, B:1, C:2);
tuple!(4; A:0, B:1, C:2, D:3);
tuple!(5; A:0, B:1, C:2, D:3, E:4);
tuple!(6; A:0, B:1, C:2, D:3, E:4, F:5);
tuple!(7; A:0, B:1, C:2, D:3, E:4, F:5, G:6);
tuple!(8; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7);
tuple!(9; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8);
tuple!(10; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9);
tuple!(11; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10);
tuple!(12; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11);
