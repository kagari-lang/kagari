use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        payload::NativeObject,
        storage::NativePayload,
        types::Type,
    },
    value::Value,
};
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};

impl<T: NativePayload> KagariType for NativeObject<T> {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(RuntimeError::module_validation(
            "native object requires a registered contextual type",
        ))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        expected.validate(cx.runtime())?;
        let Ty::NativeObject(nominal) = expected.ty() else {
            return Err(RuntimeError::module_validation(
                "native object requires an opaque storage type",
            ));
        };
        if cx
            .runtime()
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .is_some_and(|storage| {
                storage.layout() == NativeStorageLayout::Opaque && storage.accepts_payload::<T>()
            })
        {
            Ok(())
        } else {
            Err(RuntimeError::module_validation(
                "native object payload mapping differs from its registration",
            ))
        }
    }
}

impl<T: NativePayload> FromKagari for NativeObject<T> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        Self::check_type(cx, expected)?;
        cx.check_value(expected, value)?;
        let Value::GcHandle(id) = value else {
            return Err(RuntimeError::module_validation("native object conversion"));
        };
        cx.runtime().gc().with_native::<T, _>(*id, |_| Ok(()))?;
        let native_type = cx
            .runtime()
            .prepare_native_type(cx.owner(), expected.clone())?;
        let root = cx
            .runtime()
            .root_value(value.clone())
            .ok_or_else(|| RuntimeError::module_validation("native object retention"))?;
        Ok(Self { root, native_type })
    }
}

impl<T: NativePayload> IntoKagari for NativeObject<T> {
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
