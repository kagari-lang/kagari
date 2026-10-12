//! Checked receiver semantics for library-authored interface protocols.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, interfaces::Interface, typed::NativeContext},
    value::Value,
    value_semantics,
};

impl<S> Interface<S> {
    /// Compare the retained object receivers using their checked equality selection.
    /// The views must have compatible declared types. Foreign/expired handles and
    /// value receivers fail; executing a selected method requires a script backend.
    /// This grants no Eq/Hash evidence to the interface's script type.
    pub fn receiver_equal(&self, cx: &mut NativeContext<'_>, other: &Self) -> NativeResult<bool> {
        cx.poll()?;
        let runtime = cx.runtime();
        runtime.gc().ensure_no_native_borrow()?;
        if !self
            .argument
            .view(&self.owner)
            .compatible(other.argument.view(&other.owner))
        {
            return Err(RuntimeError::module_validation(
                "receiver comparison view mismatch",
            ));
        }
        let invalid = || RuntimeError::module_validation("foreign or expired interface handle");
        let left = self.root.value(runtime.gc()).ok_or_else(invalid)?;
        let right = other.root.value(runtime.gc()).ok_or_else(invalid)?;
        match runtime.compare_interface_values(left, right, cx.invoke_script)? {
            Value::Bool(result) => Ok(result),
            _ => Err(RuntimeError::module_validation(
                "receiver comparison result",
            )),
        }
    }

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
