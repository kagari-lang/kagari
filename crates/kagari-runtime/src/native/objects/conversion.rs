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
    value::view::ValueView,
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
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        Self::check_type(cx, expected)?;
        cx.check_value(expected, value)?;
        let layout = cx.struct_layout(value)?;
        let origin = cx.owner_origin();
        let object_type = cx
            .runtime_mut()
            .retain_object_type(layout.clone(), origin)?;
        let root = cx.retain(value)?;
        Ok(Self {
            root,
            object_type,
            layout,
            schema: PhantomData,
        })
    }
}

impl<S: KagariType> IntoKagari for Object<S> {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        Self::check_type(cx, expected)?;
        let value = cx.import(&self.root)?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
