//! Native task providers receive admission and cancellation, never a driver.
use crate::{
    gc::roots::RootedValue,
    native::{binding::NativeResult, context::CallContext},
    task::SpawnError,
    value::Value,
};

impl CallContext<'_> {
    /// Admit a cold factory into a checked scope without executing its body.
    pub fn spawn_task(
        &self,
        scope: &Value,
        factory: &Value,
    ) -> NativeResult<Result<RootedValue, SpawnError>> {
        self.runtime.spawn_task(scope, factory)
    }

    /// Request cancellation without driving a task or detaching its dependents.
    pub fn cancel_task(&self, task: &Value) -> NativeResult<()> {
        self.runtime.cancel_task(task)
    }
}
