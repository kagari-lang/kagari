//! Execution authority, budgets and entry policy.
use crate::{
    RunResult,
    error::{EmbeddingError, RuntimeFailureKind},
};

use kagari_common::cancellation::CancellationToken;
use kagari_runtime::{
    resource::ResourcePolicy,
    session::{DeterministicInputs, ExecutionOptions, ExecutionPhase},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JitPolicy {
    #[default]
    Disabled,
    Enabled,
    CompileOnLoad,
    CompileOnFirstCall,
    CompileAfterThreshold(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanicPolicy {
    Propagate,
    #[default]
    ConvertToError,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionContext {
    pub cancellation: CancellationToken,

    pub resources: ResourcePolicy,

    pub jit_policy: JitPolicy,
    pub tracing_enabled: bool,
    pub inputs: DeterministicInputs,
    pub panic_policy: PanicPolicy,
}

impl ExecutionContext {
    pub(crate) fn runtime_options(&self) -> ExecutionOptions {
        ExecutionOptions {
            phase: ExecutionPhase::Ordinary,

            resources: self.resources,
            cancellation: self.cancellation.clone(),
            inputs: self.inputs,
            record_host_calls: self.tracing_enabled,
        }
    }
    pub(crate) fn validate_for_execute(&self, entry: &str) -> RunResult<()> {
        if self.jit_policy != JitPolicy::Disabled {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!("JIT policy for `{entry}` is not implemented by the baseline runtime"),
            ));
        }
        Ok(())
    }
}
