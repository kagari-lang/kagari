//! Bounded formatting for library-authored interface protocols.
use crate::{
    native::{binding::NativeResult, interfaces::Interface, typed::NativeContext},
    value_semantics,
};

impl<S> Interface<S> {
    /// Produce the bounded interface preview without invoking user formatting.
    /// The owning handle is checked against this runtime before reading its value.
    pub fn debug_preview(&self, cx: &mut NativeContext<'_>) -> NativeResult<String> {
        cx.poll()?;
        value_semantics::format_value(cx.runtime_mut(), &self.root, true)
    }
}
