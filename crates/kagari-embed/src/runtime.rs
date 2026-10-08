//! Host-facing runtime linking and execution orchestration.
pub mod owned;
mod typed;
use crate::{
    LoadResult, ReloadResult, RunResult,
    context::ExecutionContext,
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
};

use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    host::{HostFunction, HostFunctionId, HostTypeRegistration},
    metadata::TypeId,
    module::LoadedModule,
    value::Value,
};
use kagari_vm::{
    error::VmError,
    vm::{ExecutionReport, Vm, native::PreparedNativeEntry},
};

#[derive(Debug)]
pub struct KagariRuntime {
    vm: Vm,
    default_context: ExecutionContext,
}

impl KagariRuntime {
    pub fn new(runtime: Runtime, default_context: ExecutionContext) -> Self {
        Self {
            vm: Vm::new(runtime),
            default_context,
        }
    }

    pub fn runtime(&self) -> &Runtime {
        self.vm.runtime()
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.vm.runtime_mut()
    }

    pub fn default_context(&self) -> &ExecutionContext {
        &self.default_context
    }

    pub fn register_host_function(
        &mut self,
        function: HostFunction,
    ) -> Result<HostFunctionId, RuntimeError> {
        self.vm.runtime_mut().register_host_function(function)
    }

    pub fn register_host_type(
        &mut self,
        registration: HostTypeRegistration,
    ) -> Result<TypeId, RuntimeError> {
        self.vm.runtime_mut().register_host_type(registration)
    }

    pub fn register_host_types(
        &mut self,
        registrations: Vec<HostTypeRegistration>,
    ) -> Result<Vec<TypeId>, RuntimeError> {
        self.vm.runtime_mut().register_host_types(registrations)
    }

    pub fn load_program(
        &mut self,
        program: &PreparedProgram,
        options: LoadOptions,
    ) -> LoadResult<LoadedModule> {
        let module_name = options.module_name.unwrap_or_else(|| {
            program.bytecode().modules()[program.bytecode().root().index()]
                .source_name
                .clone()
        });
        self.vm
            .runtime_mut()
            .load_verified_program(module_name, program.bytecode().clone())
            .map_err(EmbeddingError::load)
    }

    pub fn reload_program(
        &self,
        previous: &LoadedModule,
        program: &PreparedProgram,
        options: ReloadOptions,
    ) -> ReloadResult<LoadedModule> {
        let module_name = options.module_name.unwrap_or_else(|| previous.name.clone());
        let candidate = self
            .vm
            .runtime()
            .stage_reload_verified_program(previous, module_name, program.bytecode().clone())
            .map_err(EmbeddingError::reload_validation)?;
        self.vm
            .runtime()
            .publish_staged_reload(candidate)
            .map_err(EmbeddingError::reload_validation)
    }

    pub fn execute(
        &self,
        module: &LoadedModule,
        entry: &str,
        args: &[Value],
        context: &ExecutionContext,
    ) -> RunResult<ExecutionReport> {
        if !args.is_empty() {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!(
                    "entry `{entry}` received {} arguments, but argument passing is not implemented",
                    args.len()
                ),
            ));
        }
        context.validate_for_execute(entry)?;
        let _session = self
            .vm
            .runtime()
            .begin_execution(module, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))?;
        self.vm.execute(module, entry).map_err(EmbeddingError::vm)
    }

    pub fn execute_prepared(
        &self,
        module: &LoadedModule,
        entry: &str,
        args: &[Value],
        context: &ExecutionContext,
        prepared: &PreparedNativeEntry,
    ) -> RunResult<ExecutionReport> {
        if !args.is_empty() {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!(
                    "entry `{entry}` received {} arguments, but argument passing is not implemented",
                    args.len()
                ),
            ));
        }

        let _session = self
            .vm
            .runtime()
            .begin_execution(module, context.runtime_options())
            .map_err(|error| EmbeddingError::vm(VmError::RuntimeError(error)))?;
        self.vm
            .execute_prepared(module, entry, prepared)
            .map_err(EmbeddingError::vm)
    }
}

#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub module_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ReloadOptions {
    pub module_name: Option<String>,
}

#[cfg(all(test, feature = "source", feature = "native"))]
mod language_contract;
