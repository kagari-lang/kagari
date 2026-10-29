//! Native task providers receive admission and cancellation, never a driver.
use crate::{
    gc::roots::RootedValue,
    native::{binding::NativeResult, context::CallContext},
    task::SpawnError,
    value::Value,
};

impl CallContext<'_> {
    /// Admit a task from live provider scope/factory transport.
    ///
    /// # Safety
    /// Keep both inputs protected until admission returns an owning task handle.
    #[doc(hidden)]
    pub unsafe fn spawn_task_transport(
        &self,
        scope: &Value,
        factory: &Value,
    ) -> NativeResult<Result<RootedValue, SpawnError>> {
        self.spawn_task(scope, factory)
    }

    /// Admit a cold factory into a checked scope without executing its body.
    pub(crate) fn spawn_task(
        &self,
        scope: &Value,
        factory: &Value,
    ) -> NativeResult<Result<RootedValue, SpawnError>> {
        self.runtime.spawn_task(scope, factory)
    }

    /// Cancel a task identified by live provider transport.
    ///
    /// # Safety
    /// Keep the task protected through cancellation admission.
    #[doc(hidden)]
    pub unsafe fn cancel_task_transport(&self, task: &Value) -> NativeResult<()> {
        self.cancel_task(task)
    }

    /// Request cancellation without driving a task or detaching its dependents.
    pub(crate) fn cancel_task(&self, task: &Value) -> NativeResult<()> {
        self.runtime.cancel_task(task)
    }
}
