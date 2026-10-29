//! Inputs borrow either an owning host handle or an existing native argument root.
use kagari_runtime::{
    native::{
        binding::NativeResult, context::CallContext, storage::access::StorageAccess,
        storage_type::StorageType, typed::NativeContext, value_handle::ScriptValue,
    },
    value::Value,
};

#[derive(Clone, Copy)]
pub(crate) enum Input<'value, 'call> {
    Retained(&'value ScriptValue),
    Argument(&'value CallContext<'call>, usize),
}

impl Input<'_, '_> {
    pub(crate) fn is_scalar(self, cx: &NativeContext<'_>) -> NativeResult<bool> {
        let value = match self {
            // SAFETY: Only the discriminant is read; no callbacks or raw copies escape.
            Self::Retained(value) => unsafe { value.value_transport(cx.runtime()) }?,
            // SAFETY: Only the discriminant is inspected while the argument frame is live.
            Self::Argument(call, index) => unsafe { call.argument_transport(index) }?,
        };
        Ok(matches!(
            value,
            Value::Unit
                | Value::Bool(_)
                | Value::Char(_)
                | Value::I32(_)
                | Value::I64(_)
                | Value::U64(_)
        ))
    }

    pub(crate) fn publish(
        self,
        access: &mut StorageAccess<'_>,
        contract: &StorageType,
    ) -> NativeResult<Value> {
        match self {
            Self::Retained(value) => access.publish(contract, value),
            Self::Argument(call, index) => access.publish_argument(contract, call, index),
        }
    }

    pub(crate) fn retain(
        self,
        access: &mut StorageAccess<'_>,
        contract: &StorageType,
    ) -> NativeResult<ScriptValue> {
        match self {
            Self::Retained(value) => Ok(value.clone()),
            Self::Argument(..) => {
                let value = self.publish(access, contract)?;
                access.retain(contract, &value)
            }
        }
    }
}
