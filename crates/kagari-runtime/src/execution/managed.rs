use crate::{
    Runtime, context::ExecutionContext, module::InstalledProgram, native::binding::NativeResult,
    scheduler::TurnBudget, task::control::TaskScopeOwner,
};
use std::task::{Context, Poll};
impl Runtime {
    /// Drive one bounded turn; idle open runtimes remain pending.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<NativeResult<()>> {
        self.poll_scheduler(cx, TurnBudget::default(), None)
    }

    /// Create a managed scope driven by this Runtime scheduler.
    pub fn create_task_scope(&mut self, module: &InstalledProgram) -> NativeResult<TaskScopeOwner> {
        self.create_task_scope_with_context(module, &self.default_context.clone())
    }

    pub fn create_task_scope_with_context(
        &mut self,
        module: &InstalledProgram,
        context: &ExecutionContext,
    ) -> NativeResult<TaskScopeOwner> {
        context.validate_for_execute("TaskScope")?;
        let module = self.select_program(module)?;
        self.create_scheduled_task_scope(&module, context.runtime_options())
    }
}
