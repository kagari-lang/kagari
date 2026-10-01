//! Execution authority, budgets and entry policy.
use crate::{
    RunResult,
    error::{EmbeddingError, RuntimeFailureKind},
};

use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, RuntimeHelper},
    module::BytecodeModule,
};
use kagari_common::cancellation::CancellationToken;
use std::rc::Rc;
use {
    kagari_common::capability::CapabilitySet,
    kagari_runtime::{
        resource::ResourcePolicy,
        security::{HostExposurePolicy, LanguageProfile, SecurityContext},
        session::{DeterministicInputs, ExecutionOptions, ExecutionPhase},
    },
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
    pub language_profile: LanguageProfile,
    pub capabilities: CapabilitySet,
    pub resources: ResourcePolicy,
    pub host_policy: HostExposurePolicy,
    pub jit_policy: JitPolicy,
    pub tracing_enabled: bool,
    pub inputs: DeterministicInputs,
    pub panic_policy: PanicPolicy,
}

impl ExecutionContext {
    pub(crate) fn runtime_options(&self) -> ExecutionOptions {
        ExecutionOptions {
            phase: ExecutionPhase::Ordinary,
            security: self.security_context(),
            host_exposure: Rc::new(self.host_policy.clone()),
            resources: self.resources,
            cancellation: self.cancellation.clone(),
            inputs: self.inputs,
            record_host_calls: self.tracing_enabled,
        }
    }
    pub fn security_context(&self) -> SecurityContext {
        SecurityContext {
            profile: self.language_profile,
            capabilities: self.capabilities,
        }
    }

    pub(crate) fn validate_for_execute(
        &self,
        entry: &str,
        module: &BytecodeModule,
    ) -> RunResult<()> {
        if self.jit_policy != JitPolicy::Disabled {
            if !self.security_context().allows_jit() {
                return Err(EmbeddingError::runtime(
                    RuntimeFailureKind::CapabilityDenied,
                    "JIT execution is denied by execution context",
                ));
            }
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!("JIT policy for `{entry}` is not implemented by the baseline runtime"),
            ));
        }
        self.validate_bytecode_policy(module)
    }

    pub(crate) fn validate_for_backend_execute(
        &self,
        _entry: &str,
        module: &BytecodeModule,
    ) -> RunResult<()> {
        if !self.security_context().allows_jit() {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::CapabilityDenied,
                "JIT execution is denied by execution context",
            ));
        }
        self.validate_bytecode_policy(module)
    }

    fn validate_bytecode_policy(&self, module: &BytecodeModule) -> RunResult<()> {
        for declaration in &module.host_interface.functions {
            if !self.host_policy.exposes_host_function(&declaration.symbol)
                || !self.security_context().allows_host_calls()
            {
                return Err(EmbeddingError::runtime(
                    RuntimeFailureKind::CapabilityDenied,
                    format!(
                        "host function '{}' is denied by execution context",
                        declaration.symbol
                    ),
                ));
            }
        }
        let security = self.security_context();
        for function in &module.functions {
            for instruction in &function.instructions {
                match instruction {
                    BytecodeInstruction::Call {
                        callee: CallTarget::RuntimeHelper(helper),
                        ..
                    } => match helper {
                        RuntimeHelper::ReflectTypeOf if !security.allows_reflection_metadata() => {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "reflection metadata is denied by execution context",
                            ));
                        }
                        RuntimeHelper::ReflectGetField(_) if !security.allows_reflection_read() => {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "reflective read is denied by execution context",
                            ));
                        }
                        RuntimeHelper::ReflectSetField(_) | RuntimeHelper::ReflectSetIndex
                            if !security.allows_reflection_write() =>
                        {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "reflection write is denied by execution context",
                            ));
                        }
                        RuntimeHelper::DynamicCall if !security.allows_dynamic_invocation() => {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "dynamic invocation is denied by execution context",
                            ));
                        }
                        _ => {}
                    },
                    BytecodeInstruction::SetPath { .. }
                    | BytecodeInstruction::ModifyPath { .. }
                        if !self.host_policy.exposes_host_path_mutation() =>
                    {
                        return Err(EmbeddingError::runtime(
                            RuntimeFailureKind::CapabilityDenied,
                            "host path mutation is denied by execution context",
                        ));
                    }
                    BytecodeInstruction::ReadPath { .. }
                    | BytecodeInstruction::MakePathView { .. }
                        if !self.host_policy.exposes_host_path_read() =>
                    {
                        return Err(EmbeddingError::runtime(
                            RuntimeFailureKind::CapabilityDenied,
                            "host path read is denied by execution context",
                        ));
                    }
                    BytecodeInstruction::SetPath { .. }
                    | BytecodeInstruction::ModifyPath { .. }
                        if !security.allows_path_mutation() =>
                    {
                        return Err(EmbeddingError::runtime(
                            RuntimeFailureKind::CapabilityDenied,
                            "host path mutation is denied by runtime capabilities",
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}
