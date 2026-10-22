//! Executor-neutral runtime turns; typed invocation futures are a separate facade.
use crate::{RunResult, context::ExecutionContext, error::EmbeddingError, runtime::Runtime};
use kagari_runtime::{
    module::LoadedModule,
    scheduler::{TurnBudget, tasks::TaskEvent, wake::DriverRegistration},
    task::control::TaskScopeOwner,
};
use kagari_vm::error::VmError;
use std::task::{Context, Poll};

impl Runtime {
    /// Drive one bounded turn. Open idle runtimes remain Pending. Register a
    /// reliable host waker and poll again on notification; no TaskId loop is needed.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<RunResult<()>> {
        self.poll_scheduler(cx, TurnBudget::default(), None)
    }

    /// Low-level driver integration shared with borrowed invocation futures.
    /// `driver` is a temporary route obtained from the core runtime; it preserves
    /// the external registration installed by ordinary polling.
    pub(crate) fn poll_scheduler(
        &mut self,
        cx: &mut Context<'_>,
        budget: TurnBudget,
        driver: Option<&DriverRegistration>,
    ) -> Poll<RunResult<()>> {
        self.vm
            .poll_scheduler(cx, budget, driver)
            .map(|result| result.map_err(EmbeddingError::vm))
    }

    /// Managed scopes progress through Runtime polling and retire admission slots
    /// automatically; scope owner drop requests cancellation of its independent Tasks.
    pub fn create_task_scope(&mut self, module: &LoadedModule) -> RunResult<TaskScopeOwner> {
        self.create_task_scope_with_context(module, &self.default_context.clone())
    }

    pub fn create_task_scope_with_context(
        &mut self,
        module: &LoadedModule,
        context: &ExecutionContext,
    ) -> RunResult<TaskScopeOwner> {
        context.validate_for_execute("TaskScope")?;
        self.runtime()
            .create_scheduled_task_scope(module, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))
    }

    pub fn take_task_event(&self) -> Option<TaskEvent> {
        self.runtime().take_task_event()
    }

    /// Synchronously stop admission and acknowledge local cleanup. This never
    /// waits for remote cancellation acknowledgement or executes script bodies.
    pub fn close(&mut self) -> RunResult<()> {
        self.runtime()
            .close()
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))
    }
}
