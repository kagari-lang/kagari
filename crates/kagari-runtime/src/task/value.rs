//! Retained scope capabilities for typed calls; only the separate owner controls lifetime.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
        value_handle::ScriptValue,
    },
    task::control::TaskScopeOwner,
};
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};

/// A cloneable script capability. Retaining it does not keep a scope open after
/// its TaskScopeOwner closes or drops, and does not grant scheduling authority.
#[derive(Debug, Clone)]
pub struct TaskScope(RootedValue);

impl TaskScopeOwner {
    pub fn handle(&self) -> TaskScope {
        TaskScope(self.capability.clone())
    }
}

impl KagariType for TaskScope {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(RuntimeError::module_validation(
            "TaskScope requires an installed signature",
        ))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        let declaration = cx.runtime().task_role(NativeStorageLayout::TaskScope)?;
        if matches!(expected.ty(), Ty::NativeObject(nominal) if nominal.declaration == declaration && nominal.arguments.is_empty() && nominal.associated_types.is_empty())
        {
            Ok(())
        } else {
            Err(RuntimeError::module_validation("TaskScope type mismatch"))
        }
    }
}

impl IntoKagari for TaskScope {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        Self::check_type(cx, expected)?;
        let value = self
            .0
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign TaskScope"))?;
        cx.check_value(expected, &value)?;
        Ok(ScriptValue::from_root(
            self.0,
            expected.clone(),
            cx.owner().clone(),
        ))
    }
}

impl FromKagari for TaskScope {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        let raw = value.call_argument(cx.runtime(), expected, cx.owner())?;
        let value = &raw;
        Self::check_type(cx, expected)?;
        cx.check_value(expected, value)?;
        cx.runtime()
            .root_value(*value)
            .map(Self)
            .ok_or_else(|| RuntimeError::module_validation("TaskScope retention"))
    }
}
