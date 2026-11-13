//! A retained value with its exact declared type for generic native callbacks.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        typed::NativeContext,
        types::Type,
    },
    value::{carriers::RootedValue, view::ValueView},
};

/// Forward an arbitrary declared generic value without copying its object graph
/// or exposing raw storage. This does not add an untyped Kagari value: each handle
/// keeps its exact applied type and access view, checked on every conversion.
#[derive(Debug, Clone)]
pub struct ScriptValue {
    root: RootedValue,
    argument: TypeArgument,
    owner: ExecutableModule,
}

impl ScriptValue {
    pub fn type_argument(&self) -> &TypeArgument {
        &self.argument
    }

    pub fn decode<T: FromKagari>(&self, cx: &mut NativeContext<'_>) -> NativeResult<T> {
        let mut conversion = ConversionContext::new(cx.runtime_mut(), &self.owner)?;
        conversion.check_type::<T>(&self.argument)?;
        let value = conversion.import(&self.root)?;
        conversion.decode_prepared(&self.argument, &value)
    }
}

impl KagariType for ScriptValue {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(RuntimeError::module_validation(
            "script value requires a declared contextual type",
        ))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        expected.validate(cx.runtime())
    }
}

impl FromKagari for ScriptValue {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        let root = cx.retain(value)?;
        Ok(Self {
            root,
            argument: expected.clone(),
            owner: cx.owner().clone(),
        })
    }
}

impl IntoKagari for ScriptValue {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        cx.check_type::<Self>(expected)?;
        if !self
            .argument
            .view(&self.owner)
            .compatible(expected.view(cx.owner()))
        {
            return Err(RuntimeError::module_validation(
                "script value declared type differs from its destination",
            ));
        }
        let value = cx.import(&self.root)?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
