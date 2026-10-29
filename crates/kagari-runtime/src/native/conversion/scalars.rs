use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        scalar::NativeScalar,
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
};
use kagari_types::scalar::BuiltinType;

macro_rules! scalar {
    ($($ty:ty),+) => { $(
        impl KagariType for $ty {
            fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
                Ok(Type::scalar(<Self as NativeScalar>::abi_type()))
            }

            fn snapshot_field(&self, cx: &mut ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<ScriptValue> {
                cx.encode_value(expected, *self)
            }
        }
        impl IntoKagari for $ty {
            fn into_kagari(self, cx: &mut ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<ScriptValue> {
                cx.retain_converted(expected, <Self as NativeScalar>::encode(self))
            }
        }
        impl FromKagari for $ty {
            fn from_kagari(cx: &mut ConversionContext<'_>, expected: &TypeArgument, value: &ScriptValue) -> NativeResult<Self> {
                let raw = value.call_argument(cx.runtime(), expected, cx.owner())?;
                let value = &raw;
                <Self as NativeScalar>::decode(value.clone())
            }
        }
    )+ };
}

scalar!(
    (),
    bool,
    char,
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

    fn snapshot_field(
        &self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        cx.charge_string(self.len())?;
        let mut copy = String::new();
        copy.try_reserve_exact(self.len()).map_err(|_| {
            cx.runtime()
                .resources()
                .limit("String field snapshot capacity")
        })?;
        copy.push_str(self);
        let value = cx.runtime().gc().alloc_string(copy)?;
        cx.retain_converted(expected, value)
    }
}

impl IntoKagari for String {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        cx.charge_string(self.len())?;
        let value = cx.runtime().gc().alloc_string(self)?;
        cx.retain_converted(expected, value)
    }
}

impl FromKagari for String {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        let raw = value.call_argument(cx.runtime(), expected, cx.owner())?;
        let value = &raw;
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
            .map_err(|_| cx.runtime().resources().limit("String conversion capacity"))?;
        result.push_str(&value);
        Ok(result)
    }
}
