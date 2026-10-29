//! A retained value with its exact declared type for generic native callbacks.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{
        inspection::{ValueView, inspect_value},
        roots::{RetainedValue, RootedValue},
    },
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        payload::borrowed::BorrowedPayload,
        storage::NativePayload,
        storage_type::StorageType,
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};

/// Forward an arbitrary declared generic value without copying its object graph
/// or exposing raw storage. This does not add an untyped Kagari value: each handle
/// keeps its exact applied type and access view, checked on every conversion.
/// Managed values retain roots for data and callable dependencies; scalar data
/// retains only its heap identity. Both keep lexical metadata without pinning
/// obsolete executable module state.
#[derive(Debug, Clone)]
pub struct ScriptValue {
    root: RetainedValue,
    argument: TypeArgument,
    owner: ExecutableModule,
}

impl ScriptValue {
    pub(crate) fn from_transport(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        let root = cx
            .runtime()
            .gc()
            .retain_value(*value)
            .ok_or_else(|| RuntimeError::module_validation("script value retention"))?;
        Ok(Self {
            root,
            argument: expected.clone(),
            owner: cx.owner().clone(),
        })
    }

    pub(crate) fn retained_root(&self) -> Option<&RootedValue> {
        self.root.as_root()
    }

    pub(crate) fn from_root(
        root: RootedValue,
        argument: TypeArgument,
        owner: ExecutableModule,
    ) -> Self {
        Self {
            root: RetainedValue::from_root(root),
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
        let root = runtime.gc().retain_native_value(value)?;
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

    pub(crate) fn value(&self, runtime: &Runtime) -> NativeResult<Value> {
        self.argument.validate(runtime)?;
        self.root
            .value(runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))
    }

    /// Read backend transport after checking its retained type and heap owner.
    ///
    /// # Safety
    /// Keep the referent protected through every use or destination publication.
    /// With the current checked-ID representation, detached copies may be used
    /// only for identity/expiry rejection probes that cannot dereference reclaimed
    /// storage. Such probes are not live backend inputs.
    /// Root aliases may replace the source; retain an independent snapshot before
    /// arbitrary callbacks can do so. A terminal backend handoff must publish before
    /// any collection after this handle is released. Ordinary host code uses
    /// inspect, typed conversion or owning object/collection access instead.
    #[doc(hidden)]
    pub unsafe fn value_transport(&self, runtime: &Runtime) -> NativeResult<Value> {
        self.value(runtime)
    }

    /// Inspect the retained value without exposing unrooted object transport.
    /// Retained type metadata is validated before lending a non-reentrant view. Release
    /// the view before conversions, callbacks, allocation or collection.
    pub fn inspect<R>(
        &self,
        runtime: &Runtime,
        read: impl for<'view> FnOnce(ValueView<'view>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.argument.validate(runtime)?;
        let value = self
            .root
            .value(runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))?;
        inspect_value(runtime.gc(), value, read)
    }

    pub fn type_argument(&self) -> &TypeArgument {
        &self.argument
    }

    /// Borrow a checked provider payload without constructing an owning Opaque
    /// descriptor or registering another root. The view cannot outlive this
    /// handle; each loan still checks runtime, generation and storage access.
    pub fn borrow_payload<'view, P: NativePayload>(
        &'view self,
        cx: &NativeContext<'view>,
    ) -> NativeResult<BorrowedPayload<'view, P>> {
        BorrowedPayload::from_root(cx, self.root.as_root(), &self.argument)
    }

    pub fn decode<T: FromKagari>(&self, cx: &mut NativeContext<'_>) -> NativeResult<T> {
        // The supplied argument preserves its original lexical metadata. The caller
        // lends live execution services; data alone does not pin obsolete programs.
        let mut conversion = cx.conversion_scope(cx.owner())?;
        conversion.decode_value(&self.argument, self)
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
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        let raw = value.call_argument(cx.runtime(), expected, cx.owner())?;
        cx.check_value(expected, &raw)?;
        Ok(value.clone())
    }
}

impl IntoKagari for ScriptValue {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
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
        Ok(self)
    }
}
