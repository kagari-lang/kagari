use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        payload::Opaque,
        rust_type::check_mapping,
        schema::TypeRegistration,
        storage::NativePayload,
        types::Type,
    },
    value::Value,
};
use kagari_types::ty::Ty;

impl<T: NativePayload> KagariType for Opaque<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        registration.opaque::<T>()
    }

    fn snapshot_field(
        &self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.encode_value(expected, self.clone())
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        if !matches!(expected.ty(), Ty::NativeObject(_)) {
            return Err(RuntimeError::module_validation(
                "native object requires an opaque storage type",
            ));
        }
        check_mapping::<T>(cx.runtime(), cx.owner(), expected)
    }
}

impl<T: NativePayload> FromKagari for Opaque<T> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        Self::from_checked(cx, expected, value)
    }
}

impl<T: NativePayload> IntoKagari for Opaque<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        Self::check_type(cx, expected)?;
        let value = self.conversion(cx, expected)?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}

impl<T: Send + 'static> Opaque<T> {
    pub(crate) fn from_checked(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        check_mapping::<T>(cx.runtime(), cx.owner(), expected)?;
        cx.check_value(expected, value)?;
        let native_type =
            cx.runtime()
                .prepare_native_type_from_object(cx.owner(), expected.clone(), value)?;
        let root = cx
            .runtime()
            .root_value(*value)
            .ok_or_else(|| RuntimeError::module_validation("native object retention"))?;
        Ok(Self { root, native_type })
    }
}
