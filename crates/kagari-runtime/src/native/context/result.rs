//! Staged return protection for the pre-cutover heap. RR03 replaces the managed
//! lease with an authoritative execution-local slot; scalar staging stays inline.
use crate::{
    error::RuntimeError,
    gc::roots::RootedValue,
    native::{binding::NativeResult, context::NativeCall},
    value::{Value, ValueCategory},
};

#[derive(Default)]
pub(crate) enum PendingReturn {
    #[default]
    Unset,
    Unit,
    Scalar(Value),
    Managed(RootedValue),
}

impl NativeCall<'_> {
    /// Stage a result without changing the caller's destination. Managed results
    /// remain protected across subsequent callbacks/collection until call cleanup.
    /// A callback may stage one result; encoding/type errors still obey post-call
    /// cancellation precedence before the ordinary return commits it.
    pub fn set_result(&mut self, value: Value) -> NativeResult<()> {
        if !matches!(self.pending, PendingReturn::Unset) {
            return Err(RuntimeError::module_validation(
                "native result already staged",
            ));
        }
        self.pending = match value {
            Value::Unit => PendingReturn::Unit,
            value if value.category() == ValueCategory::Primitive => PendingReturn::Scalar(value),
            value => PendingReturn::Managed(
                self.runtime
                    .root_value(value)
                    .ok_or_else(|| RuntimeError::module_validation("native result retention"))?,
            ),
        };
        Ok(())
    }

    pub(crate) fn result(&self) -> NativeResult<Value> {
        match &self.pending {
            PendingReturn::Unset => Err(RuntimeError::module_validation("native result is unset")),
            PendingReturn::Unit => Ok(Value::Unit),
            PendingReturn::Scalar(value) => Ok(*value),
            PendingReturn::Managed(root) => root
                .value(self.heap())
                .ok_or_else(|| RuntimeError::module_validation("native result root")),
        }
    }
}
