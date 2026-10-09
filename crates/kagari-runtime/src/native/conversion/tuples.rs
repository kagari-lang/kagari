use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};
use kagari_types::ty::Ty;

macro_rules! tuple {
    ($count:expr; $($ty:ident:$slot:tt),+) => {
        impl<$($ty: KagariType),+> KagariType for ($($ty,)+) {
            fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
                Ok(Type::tuple([$($ty::kagari_type(catalog)?),+]))
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
            fn into_kagari(self, cx: &mut ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<Value> {
                cx.check_elements($count)?;
                let values = vec![$({
                    let element = cx.parameter(expected, $slot)?;
                    cx.encode_value(&element, self.$slot)?
                }),+];
                cx.runtime().gc().alloc_tuple(values)
            }
        }
        impl<$($ty: FromKagari),+> FromKagari for ($($ty,)+) {
            fn from_kagari(cx: &mut ConversionContext<'_>, expected: &TypeArgument, value: &Value) -> NativeResult<Self> {
                let Value::Tuple(elements) = value else { return Err(RuntimeError::module_validation("tuple conversion value")); };
                let elements = cx.runtime().gc().tuple(*elements).ok_or_else(|| RuntimeError::module_validation("invalid tuple"))?.to_vec();
                if elements.len() != $count { return Err(RuntimeError::module_validation("tuple conversion arity")); }
                cx.check_elements($count)?;
                Ok(($({
                    let element = cx.parameter(expected, $slot)?;
                    cx.decode_value::<$ty>(&element, &elements[$slot])?
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
