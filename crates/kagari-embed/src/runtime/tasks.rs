//! Scope task scheduling shares the owned execution backend.
use crate::{RunResult, context::ExecutionContext, error::EmbeddingError, runtime::KagariRuntime};
use kagari_runtime::{
    module::LoadedModule,
    task::{
        TaskId,
        control::{TaskDispatcher, TaskScopeOwner},
        drive::TaskDriveResult,
    },
};
use kagari_vm::error::VmError;
use std::{num::NonZeroUsize, sync::Arc};

impl KagariRuntime {
    pub fn create_task_scope(
        &self,
        module: &LoadedModule,
        context: &ExecutionContext,
        dispatcher: Arc<dyn TaskDispatcher>,
    ) -> RunResult<TaskScopeOwner> {
        context.validate_for_execute("TaskScope")?;
        self.runtime()
            .create_task_scope(module, context.runtime_options(), dispatcher)
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))
    }

    pub fn drive_task(
        &self,
        scope: &TaskScopeOwner,
        task: TaskId,
        slice: NonZeroUsize,
    ) -> RunResult<TaskDriveResult> {
        self.vm
            .drive_task(scope, task, slice)
            .map_err(EmbeddingError::vm)
    }
}
