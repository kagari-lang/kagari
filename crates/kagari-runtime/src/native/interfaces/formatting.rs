//! Bounded formatting for library-authored interface protocols.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, interfaces::Interface, typed::NativeContext},
    value_semantics,
};

impl<S> Interface<S> {
    /// Produce the bounded interface preview without invoking user formatting.
    /// The owning handle is checked against this runtime before reading its value.
    pub fn debug_preview(&self, cx: &NativeContext<'_>) -> NativeResult<String> {
        cx.poll()?;
        let value = self.root.value(cx.runtime().gc()).ok_or_else(|| {
            RuntimeError::module_validation("foreign or expired interface handle")
        })?;
        value_semantics::format_value(cx.runtime().gc(), &value, true)
    }
}
