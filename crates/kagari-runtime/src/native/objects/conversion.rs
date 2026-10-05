use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        objects::{Dynamic, Object},
        types::Type,
    },
    value::Value,
};
use kagari_types::ty::Ty;
use std::marker::PhantomData;

impl KagariType for Dynamic {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(RuntimeError::module_validation(
            "dynamic object requires an explicit installed type",
        ))
    }

    fn check_type(_: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        if matches!(expected.ty(), Ty::Struct(_)) {
            Ok(())
        } else {
            Err(RuntimeError::module_validation(
                "object conversion requires a script struct",
            ))
        }
    }
}

impl<S: KagariType> KagariType for Object<S> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        let ty = S::kagari_type(catalog)?;
        if !matches!(ty.abi(), Ty::Struct(_)) {
            return Err(RuntimeError::module_validation(
                "object schema requires a script struct",
            ));
        }
        Ok(ty)
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        Dynamic::check_type(cx, expected)?;
        S::check_type(cx, expected)
    }
}

impl<S: KagariType> FromKagari for Object<S> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        Self::check_type(cx, expected)?;
        cx.check_value(expected, value)?;
        let Value::Struct(id) = value else {
            return Err(RuntimeError::module_validation(
                "object value is not a struct",
            ));
        };
        let layout = cx
            .runtime()
            .gc()
            .struct_layout(*id)
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired object"))?;
        let object_type = cx.runtime().retain_object_type(layout.clone())?;
        let root = cx
            .runtime()
            .root_value(value.clone())
            .ok_or_else(|| RuntimeError::module_validation("object result retention"))?;
        Ok(Self {
            root,
            object_type,
            layout,
            schema: PhantomData,
        })
    }
}

impl<S: KagariType> IntoKagari for Object<S> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        Self::check_type(cx, expected)?;
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired object handle"))?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
