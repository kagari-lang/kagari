pub mod native;
pub mod owned;
mod tasks;
mod typed;
use kagari_bytecode::{
    artifact::{ArtifactCompatibility, KbcArtifact},
    module::BytecodeModule,
    program::BytecodeProgram,
};
use kagari_common::identity::{reference::DefinitionReference, table::DefinitionId};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    error_trace::ResultFailure,
    gc::roots::RootedValue,
    module::LoadedModule,
    reload::ReloadValidationError,
    session::{ExecutionSession, ExecutionTrace},
    value::Value,
};
use std::{
    cell::{Ref, RefMut},
    iter,
};
use {
    kagari_abi::native::BackendId,
    kagari_contract::{ids::FunctionRef, native::ExecutableFunctionArtifact},
};

use crate::{debug::DebugSession, error::VmError, executor::Executor};

#[derive(Debug)]
pub enum ReloadError {
    Validation(ReloadValidationError),
}

#[derive(Debug)]
pub struct Vm {
    runtime: Runtime,
}

#[derive(Debug)]
pub struct ExecutionReport {
    pub module_name: String,
    pub epoch: u64,
    pub entry: String,
    /// Owning retention transfers with this field and survives later calls/GC.
    /// Inspect through the owning runtime's heap; clones share the root lease.
    pub return_value: RootedValue,
    pub failure: Option<ResultFailure>,
    pub jit: Option<JitExecutionReport>,
    pub trace: Option<ExecutionTrace>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JitExecutionReport {
    pub backend: BackendId,
    pub function: FunctionRef,
    pub status: JitExecutionStatus,
    pub artifact: Option<ExecutableFunctionArtifact>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitExecutionStatus {
    Native,
    InterpreterFallback,
}

impl Vm {
    pub fn new(runtime: Runtime) -> Self {
        Self { runtime }
    }

    pub fn reload_program(
        &self,
        active: &LoadedModule,
        name: impl Into<String>,
        program: BytecodeProgram,
    ) -> Result<LoadedModule, ReloadError> {
        let candidate = self
            .runtime
            .stage_reload_program(active, name, program)
            .map_err(ReloadError::Validation)?;
        self.runtime
            .publish_staged_reload(candidate)
            .map_err(ReloadError::Validation)
    }

    pub fn reload_artifact(
        &self,
        active: &LoadedModule,
        name: impl Into<String>,
        artifact: KbcArtifact,
        compatibility: &ArtifactCompatibility,
    ) -> Result<LoadedModule, ReloadError> {
        let candidate = self
            .runtime
            .stage_reload_artifact(active, name, artifact, compatibility)
            .map_err(ReloadError::Validation)?;
        self.runtime
            .publish_staged_reload(candidate)
            .map_err(ReloadError::Validation)
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    pub fn attach_debug_session(&mut self, session: DebugSession) -> Result<(), VmError> {
        self.runtime.set_execution_observer(session)?;
        Ok(())
    }

    pub fn debug_session(&self) -> Option<Ref<'_, DebugSession>> {
        self.runtime.execution_observer::<DebugSession>()
    }

    pub fn debug_session_mut(&mut self) -> Option<RefMut<'_, DebugSession>> {
        self.runtime.execution_observer_mut::<DebugSession>()
    }

    fn begin_execution(&self, module: &LoadedModule) -> Result<ExecutionSession<'_>, VmError> {
        let session = self
            .runtime
            .begin_execution(module, self.runtime.execution_options())?;
        if self.runtime.execution_observer::<DebugSession>().is_some() {
            self.runtime.attach_execution_observer()?;
        }
        Ok(session)
    }

    pub fn execute(&self, module: &LoadedModule, entry: &str) -> Result<ExecutionReport, VmError> {
        let _session = self.begin_execution(module)?;
        self.runtime
            .validate_loaded_module(module)
            .map_err(VmError::RuntimeError)?;
        let entry_name = entry.to_owned();
        let entry = find_function_ref(&module.bytecode, &entry_name)?;
        let mut executor = Executor::new(&self.runtime, module, entry, &[])?;
        let return_value = executor.run()?;
        let failure = self.runtime.result_failure(&return_value);
        let return_value = self
            .runtime
            .root_value(return_value)
            .ok_or_else(|| RuntimeError::module_validation("execution result root"))?;

        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: entry_name,
            failure,
            return_value,
            jit: None,
            trace: _session.trace(),
        })
    }

    /// Invokes a linked script implementation through a runtime-owned
    /// interface value. The receiver and arguments remain rooted while module
    /// the method body executes.
    pub fn invoke_interface_method<I: DefinitionReference>(
        &self,
        interface: &Value,
        method: &I,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let resolved = self
            .runtime
            .resolve_interface_method(interface, method)
            .map_err(VmError::RuntimeError)?;
        let loaded = resolved.implementation(&self.runtime)?;
        let args = iter::once(resolved.receiver().clone())
            .chain(arguments.iter().cloned())
            .collect::<Vec<_>>();
        let _argument_roots = self
            .runtime
            .gc()
            .root_execution_values(args.clone())
            .ok_or(VmError::TypeMismatch("invalid interface method argument"))?;
        let _session = self.begin_execution(&loaded)?;
        self.runtime
            .validate_loaded_module(&loaded)
            .map_err(VmError::RuntimeError)?;
        Executor::new_interface(&self.runtime, resolved, &args)?.run()
    }
}

fn find_function_ref(
    module: &BytecodeModule<DefinitionId>,
    name: &str,
) -> Result<FunctionRef, VmError> {
    let mut matches = module
        .functions
        .iter()
        .filter(|function| function.name == name);
    let first = matches
        .next()
        .ok_or_else(|| VmError::MissingFunction(name.to_owned()))?;
    if matches.next().is_some() {
        return Err(VmError::AmbiguousFunction(name.to_owned()));
    }
    Ok(first.id)
}
