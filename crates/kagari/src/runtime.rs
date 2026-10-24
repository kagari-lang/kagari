//! Host-facing runtime linking and execution orchestration.
mod bindings;
pub mod call;
mod invocation;
mod members;

mod scheduler;
mod scoped;
mod source;
mod typed;
#[cfg(any(feature = "source", feature = "native"))]
mod update;
#[cfg(any(feature = "source", feature = "native"))]
use crate::runtime::update::Publications;
use crate::{
    LoadResult, ReloadResult, RunResult,
    context::ExecutionContext,
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
};

use kagari_runtime::{
    Runtime as CoreRuntime,
    module::{Program, entry::ProgramEntry},
    value::Value,
};
use kagari_vm::{
    error::VmError,
    vm::{
        ExecutionReport, Vm,
        native::{PreparedNativeEntry, PreparedNativeExecution},
    },
};

#[derive(Debug)]
pub struct Runtime {
    vm: Vm,
    default_context: ExecutionContext,
    #[cfg(any(feature = "source", feature = "native"))]
    publications: Publications,
}

impl Runtime {
    pub fn new(runtime: CoreRuntime, default_context: ExecutionContext) -> Self {
        Self {
            vm: Vm::new(runtime),
            default_context,
            #[cfg(any(feature = "source", feature = "native"))]
            publications: Publications::default(),
        }
    }

    /// Interpreter primitives for backend tooling and low-level contract tests.
    /// Normal async callers use `call_async`, `submit_async` and `poll` instead.
    pub fn backend(&self) -> &Vm {
        &self.vm
    }

    pub fn runtime(&self) -> &CoreRuntime {
        self.vm.runtime()
    }

    pub fn runtime_mut(&mut self) -> &mut CoreRuntime {
        self.vm.runtime_mut()
    }

    pub fn default_context(&self) -> &ExecutionContext {
        &self.default_context
    }

    pub fn load_program(
        &mut self,
        program: &PreparedProgram,
        options: LoadOptions,
    ) -> LoadResult<Program> {
        let program_name = options.program_name.unwrap_or_else(|| {
            program.bytecode().modules()[program.bytecode().root().index()]
                .source_name
                .clone()
        });
        let candidate = self
            .vm
            .runtime_mut()
            .stage_load_verified_program(program_name, program.bytecode().clone())
            .map_err(EmbeddingError::load)?;
        #[cfg(any(feature = "source", feature = "native"))]
        let association = self
            .publications
            .stage(candidate.publication_token(), program.retained_inputs())?;
        let loaded = self
            .vm
            .runtime_mut()
            .publish_staged_load(candidate)
            .map_err(EmbeddingError::load)?;
        #[cfg(any(feature = "source", feature = "native"))]
        association.published();
        Ok(loaded)
    }

    pub fn reload_program(
        &self,
        previous: &Program,
        program: &PreparedProgram,
        options: ReloadOptions,
    ) -> ReloadResult<()> {
        let guard = self.runtime().begin_publication(previous)?;
        let candidate = guard
            .stage_reload_verified_program(options.program_name, program.bytecode().clone())
            .map_err(EmbeddingError::reload_validation)?;
        #[cfg(any(feature = "source", feature = "native"))]
        let association = self
            .publications
            .stage(candidate.publication_token(), program.retained_inputs())?;
        guard
            .publish(candidate)
            .map_err(EmbeddingError::reload_validation)?;
        #[cfg(any(feature = "source", feature = "native"))]
        association.published();
        Ok(())
    }

    pub fn execute<'a>(
        &self,
        program: &Program,
        entry: impl Into<ProgramEntry<'a>>,
        args: &[Value],
        context: &ExecutionContext,
    ) -> RunResult<ExecutionReport> {
        let entry = entry.into();
        if !args.is_empty() {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!(
                    "entry `{}` received {} arguments, but argument passing is not implemented",
                    entry.label(),
                    args.len()
                ),
            ));
        }
        context.validate_for_execute(entry.label())?;
        let module = self.runtime().select_program(program)?;
        let _session = self
            .vm
            .runtime()
            .begin_execution(&module, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))?;
        self.vm.execute(&module, entry).map_err(EmbeddingError::vm)
    }

    pub fn execute_prepared<'a>(
        &self,
        program: &Program,
        entry: impl Into<ProgramEntry<'a>>,
        args: &[Value],
        context: &ExecutionContext,
        prepared: &PreparedNativeEntry,
    ) -> RunResult<ExecutionReport> {
        let entry = entry.into();
        if !args.is_empty() {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!(
                    "entry `{}` received {} arguments, but argument passing is not implemented",
                    entry.label(),
                    args.len()
                ),
            ));
        }

        let module = self.runtime().select_program(program)?;
        let _session = self
            .vm
            .runtime()
            .begin_execution(&module, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))?;
        self.vm
            .execute_prepared(
                &module,
                entry,
                PreparedNativeExecution {
                    prepared,
                    fallback_reason: context.jit_policy.fallback_reason(),
                },
            )
            .map_err(EmbeddingError::vm)
    }
}

#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub program_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ReloadOptions {
    pub program_name: Option<String>,
}

#[cfg(all(test, feature = "source", feature = "native"))]
mod language_contract;

#[cfg(test)]
mod tests;
