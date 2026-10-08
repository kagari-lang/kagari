//! Short host-driven activations of runtime-owned executions.
use crate::{RunResult, context::ExecutionContext, error::EmbeddingError, runtime::KagariRuntime};
use kagari_runtime::{
    gc::roots::RootedValue, module::LoadedModule, session::owned::OwnedExecution, value::Value,
};
use kagari_vm::{error::VmError, vm::owned::DriveResult as VmDriveResult};
use std::num::NonZeroUsize;

/// API errors are returned by `drive`; a terminal script failure belongs to Complete.
#[derive(Debug)]
pub enum DriveResult {
    Runnable,
    Waiting,
    Complete(RunResult<RootedValue>),
}

impl KagariRuntime {
    /// Queue a pinned entry and retain its arguments without executing its body.
    /// The baseline owned driver is interpreted. Native preparation of a resume
    /// body reports Unsupported before compilation; select this path explicitly.
    pub fn start(
        &self,
        module: &LoadedModule,
        entry: &str,
        arguments: &[Value],
        context: &ExecutionContext,
    ) -> RunResult<OwnedExecution> {
        context.validate_for_execute(entry)?;
        self.vm
            .start(module, entry, arguments, context.runtime_options())
            .map_err(EmbeddingError::vm)
    }

    /// Drive one positive instruction slice. A Waiting result retains no Runtime
    /// borrow; register a host waker on the owner and drive again when it is ready.
    /// Wakes are scheduling hints, never callbacks into this driver.
    pub fn drive(&self, execution: &OwnedExecution, slice: NonZeroUsize) -> RunResult<DriveResult> {
        Ok(
            match self
                .vm
                .drive(execution, slice)
                .map_err(EmbeddingError::vm)?
            {
                VmDriveResult::Runnable => DriveResult::Runnable,
                VmDriveResult::Waiting => DriveResult::Waiting,
                VmDriveResult::Complete(result) => {
                    DriveResult::Complete(result.map_err(EmbeddingError::vm))
                }
            },
        )
    }

    /// Finish cleanup requested by dropped owners, including provider cancellation.
    /// A cancellation request on a retained owner completes through its next drive.
    pub fn drain_retired_executions(&self) -> RunResult<usize> {
        self.runtime()
            .drain_retired_executions()
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))
    }
}
