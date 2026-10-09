use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        scalar::NativeScalar,
        types::Type,
    },
    value::Value,
};
use kagari_types::scalar::BuiltinType;

macro_rules! scalar {
    ($($ty:ty),+) => { $(
        impl KagariType for $ty {
            fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
                Ok(Type::from_semantic(<Self as NativeScalar>::abi_type()))
            }
        }
        impl IntoKagari for $ty {
            fn into_kagari(self, _: &mut ConversionContext<'_>, _: &TypeArgument) -> NativeResult<Value> {
                Ok(<Self as NativeScalar>::encode(self))
            }
        }
        impl FromKagari for $ty {
            fn from_kagari(_: &mut ConversionContext<'_>, _: &TypeArgument, value: &Value) -> NativeResult<Self> {
                <Self as NativeScalar>::decode(value.clone())
            }
        }
    )+ };
}

scalar!(
    (),
    bool,
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    f32,
    f64
);

impl KagariType for String {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Ok(Type::scalar(BuiltinType::String))
    }
}

impl IntoKagari for String {
    fn into_kagari(self, cx: &mut ConversionContext<'_>, _: &TypeArgument) -> NativeResult<Value> {
        cx.charge_string(self.len())?;
        cx.runtime().gc().alloc_string(self)
    }
}

impl FromKagari for String {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        _: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let Value::Str(value) = value else {
            return Err(RuntimeError::module_validation(
                "String conversion requires a string",
            ));
        };
        let runtime = cx.runtime();
        let value = runtime
            .gc()
            .string(*value)
            .ok_or_else(|| RuntimeError::module_validation("invalid string"))?;
        cx.charge_string(value.len())?;
        let mut result = String::new();
        result
            .try_reserve_exact(value.len())
            .map_err(|_| RuntimeError::resource_limit("String conversion capacity"))?;
        result.push_str(&value);
        Ok(result)
    }
}
