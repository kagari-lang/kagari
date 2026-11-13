//! Native task services admit actual sources; only queued state escapes ownership.
use crate::{
    native::{binding::NativeResult, context::NativeCall, conversion::context::inputs::ValueInput},
    task::SpawnError,
    value::view::ValueView,
};

impl<'call> NativeCall<'call> {
    /// Admit a source-protected cold factory without executing its body.
    /// The queued factory owns its retained closure; capability reads add no root.
    pub fn spawn_task(
        &mut self,
        scope: &ValueView<'_>,
        factory: &ValueView<'_>,
    ) -> NativeResult<Result<ValueView<'call>, SpawnError>> {
        let task = self
            .runtime
            .spawn_task_inner(ValueInput::View(*scope), |runtime| {
                runtime.prepare_future_factory_view(factory, None)
            })?;
        match task {
            Ok(task) => self
                .convert_value(|conversion| conversion.import(&task))
                .map(Ok),
            Err(error) => Ok(Err(error)),
        }
    }

    /// Request cancellation without driving a task or registering a Host root.
    pub fn cancel_task(&mut self, task: &ValueView<'_>) -> NativeResult<()> {
        self.runtime.cancel_task_input(ValueInput::View(*task))
    }
}
