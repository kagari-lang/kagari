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
    value::{kind::ObjectKind, view::ValueView},
};
use kagari_types::scalar::BuiltinType;

macro_rules! scalar {
    ($($ty:ty),+) => { $(
        impl KagariType for $ty {
            fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
                Ok(Type::scalar(<Self as NativeScalar>::abi_type()))
            }

            fn snapshot_field<'call>(&self, cx: &mut ConversionContext<'call>, expected: &TypeArgument) -> NativeResult<ValueView<'call>> {
                cx.encode_value(expected, *self)
            }
        }

        impl IntoKagari for $ty {
            fn into_kagari<'call>(self, _: &mut ConversionContext<'call>, _: &TypeArgument) -> NativeResult<ValueView<'call>> {
                Ok(<Self as NativeScalar>::encode(self))
            }
        }

        impl FromKagari for $ty {
            fn from_kagari(_: &mut ConversionContext<'_>, _: &TypeArgument, value: &ValueView<'_>) -> NativeResult<Self> {
                <Self as NativeScalar>::decode(*value)
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

    fn snapshot_field<'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        cx.charge_string(self.len())?;
        let mut copy = String::new();
        copy.try_reserve_exact(self.len()).map_err(|_| {
            cx.runtime()
                .resources()
                .limit("String field snapshot capacity")
        })?;
        copy.push_str(self);
        let value = cx.allocate_string(copy)?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}

impl IntoKagari for String {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        _: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        cx.charge_string(self.len())?;
        cx.allocate_string(self)
    }
}

impl FromKagari for String {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        _: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        if value.kind() != Some(ObjectKind::String) {
            return Err(RuntimeError::module_validation(
                "String conversion requires a string",
            ));
        }
        let len = cx.string_len(value)?;
        cx.charge_string(len)?;
        let mut result = String::new();
        result
            .try_reserve_exact(len)
            .map_err(|_| cx.runtime().resources().limit("String conversion capacity"))?;
        cx.read_string(value, |text| result.push_str(text))?;
        Ok(result)
    }
}
