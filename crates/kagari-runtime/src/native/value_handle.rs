//! A retained value with its exact declared type for generic native callbacks.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        storage_type::StorageType,
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};

/// Forward an arbitrary declared generic value without copying its object graph
/// or exposing raw storage. This does not add an untyped Kagari value: each handle
/// keeps its exact applied type and access view, checked on every conversion.
/// The root retains data and callable dependencies; ordinary data keeps lexical
/// metadata without keeping obsolete executable module state alive.
#[derive(Debug, Clone)]
pub struct ScriptValue {
    root: RootedValue,
    argument: TypeArgument,
    owner: ExecutableModule,
}

impl ScriptValue {
    pub(crate) fn from_root(
        root: RootedValue,
        argument: TypeArgument,
        owner: ExecutableModule,
    ) -> Self {
        Self {
            root,
            argument,
            owner,
        }
    }

    pub(crate) fn call_argument(
        &self,
        runtime: &Runtime,
        expected: &TypeArgument,
        owner: &ExecutableModule,
    ) -> NativeResult<Value> {
        self.argument.validate(runtime)?;
        expected.validate(runtime)?;
        if !self
            .argument
            .view(&self.owner)
            .compatible(expected.view(owner))
        {
            return Err(RuntimeError::module_validation(
                "retained call argument type",
            ));
        }
        self.root.value(runtime.gc()).ok_or_else(|| {
            RuntimeError::module_validation("foreign or expired retained call argument")
        })
    }

    pub(crate) fn retain_storage(
        runtime: &Runtime,
        contract: &StorageType,
        value: Value,
    ) -> NativeResult<Self> {
        let argument = contract.argument()?.clone();
        let root = runtime.gc().root_native_value(value)?;
        Ok(Self {
            root,
            argument,
            owner: contract.owner.clone(),
        })
    }

    pub(crate) fn storage_value(
        &self,
        runtime: &Runtime,
        contract: &StorageType,
    ) -> NativeResult<Value> {
        if !contract.matches_view(self.argument.view(&self.owner)) {
            return Err(RuntimeError::module_validation(
                "script value declared type differs from storage",
            ));
        }
        self.root
            .value(runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))
    }

    /// Read the checked value while this handle's root remains alive. The copy
    /// must not outlive that protection unless the caller retains another root.
    pub fn value(&self, runtime: &Runtime) -> NativeResult<Value> {
        self.argument.validate(runtime)?;
        self.root
            .value(runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))
    }

    pub fn type_argument(&self) -> &TypeArgument {
        &self.argument
    }

    pub fn decode<T: FromKagari>(&self, cx: &mut NativeContext<'_>) -> NativeResult<T> {
        // The supplied argument preserves its original lexical metadata. The caller
        // lends live execution services; data alone does not pin obsolete programs.
        let mut conversion = ConversionContext::new(cx.runtime(), cx.owner())?;
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))?;
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
        value: &Value,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        let root = cx
            .runtime()
            .root_value(*value)
            .ok_or_else(|| RuntimeError::module_validation("script value retention"))?;
        Ok(Self {
            root,
            argument: expected.clone(),
            owner: cx.owner().clone(),
        })
    }
}

impl IntoKagari for ScriptValue {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
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
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
