//! Execution cancellation, deterministic inputs and entry policy.
use crate::{
    error::{RuntimeError, reason::ErrorReason},
    native::binding::NativeResult,
};

use crate::session::{DeterministicInputs, ExecutionOptions, ExecutionPhase};
use kagari_common::cancellation::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JitPolicy {
    #[default]
    Disabled,
    Enabled,
    CompileOnLoad,
    CompileOnFirstCall,
    CompileAfterThreshold(u32),
}

impl JitPolicy {
    pub fn fallback_reason(self) -> Option<&'static str> {
        match self {
            Self::Disabled => Some("JIT disabled by execution context policy"),
            Self::Enabled
            | Self::CompileOnLoad
            | Self::CompileOnFirstCall
            | Self::CompileAfterThreshold(_) => None,
        }
    }
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

    pub jit_policy: JitPolicy,
    pub tracing_enabled: bool,
    pub inputs: DeterministicInputs,
    pub panic_policy: PanicPolicy,
}

impl ExecutionContext {
    /// Validate interpreter policy and preserve invocation inputs for backend tooling.
    /// Typed SDK calls perform this validation themselves.
    pub fn interpreter_options(&self) -> NativeResult<ExecutionOptions> {
        self.validate_for_execute("interpreter entry")?;
        Ok(self.runtime_options())
    }

    /// Project cancellation, deterministic inputs and observation into driver options.
    /// This does not validate interpreter/JIT policy or admit an execution.
    pub fn runtime_options(&self) -> ExecutionOptions {
        ExecutionOptions {
            phase: ExecutionPhase::Ordinary,

            cancellation: self.cancellation.clone(),
            inputs: self.inputs,
            record_host_calls: self.tracing_enabled,
        }
    }

    pub(crate) fn validate_for_execute(&self, entry: &str) -> NativeResult<()> {
        if self.jit_policy != JitPolicy::Disabled {
            return Err(RuntimeError::new(ErrorReason::UnsupportedExecution(
                format!("JIT policy for `{entry}` is not implemented by the baseline runtime"),
            )));
        }
        Ok(())
    }
}
