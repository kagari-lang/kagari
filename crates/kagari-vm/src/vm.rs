#[cfg(test)]
pub(crate) mod fixtures;
pub mod native;
pub mod owned;
mod scheduler;
mod tasks;
mod typed;
use kagari_bytecode::{
    artifact::{ArtifactCompatibility, KbcArtifact},
    module::CallableTarget,
    program::BytecodeProgram,
};
use kagari_common::identity::{metadata::DefinitionMetadata, table::DefinitionId};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    error_trace::ResultFailure,
    gc::roots::RootedValue,
    module::{ExecutableModule, Program, entry::ProgramEntry},
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
        active: &Program,
        name: Option<String>,
        program: DefinitionMetadata<BytecodeProgram>,
    ) -> Result<(), ReloadError> {
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
        active: &Program,
        name: Option<String>,
        artifact: DefinitionMetadata<KbcArtifact>,
        compatibility: &ArtifactCompatibility,
    ) -> Result<(), ReloadError> {
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

    fn begin_execution(&self, module: &ExecutableModule) -> Result<ExecutionSession<'_>, VmError> {
        let session = self
            .runtime
            .begin_execution(module, self.runtime.execution_options())?;
        if self.runtime.execution_observer::<DebugSession>().is_some() {
            self.runtime.attach_execution_observer()?;
        }
        Ok(session)
    }

    /// Execute a public path or the explicitly selected zero-argument root entry.
    pub fn execute<'a>(
        &self,
        module: &ExecutableModule,
        entry: impl Into<ProgramEntry<'a>>,
    ) -> Result<ExecutionReport, VmError> {
        let entry = entry.into();
        self.runtime
            .validate_executable_module(module)
            .map_err(VmError::RuntimeError)?;
        let entry_name = entry.label().to_owned();
        let (module, function) = entry
            .resolve(module)
            .ok_or_else(|| VmError::MissingFunction(entry_name.clone()))?;
        let (module, function) = self.select_function(&module, function)?;
        let session = self.begin_execution(&module)?;
        self.execute_selected(&module, function, &entry_name, &session)
    }

    fn select_function(
        &self,
        module: &ExecutableModule,
        function: FunctionRef,
    ) -> Result<(ExecutableModule, FunctionRef), VmError> {
        self.runtime.validate_executable_module(module)?;
        let route = module.bind_call_route(CallableTarget::Script(function))?;
        let selected = self.runtime.select_call_route(&route)?;
        let CallableTarget::Script(function) = selected.target() else {
            return Err(
                RuntimeError::module_validation("script entry selected a native import").into(),
            );
        };
        Ok((selected.owner().clone(), function))
    }

    fn execute_selected(
        &self,
        module: &ExecutableModule,
        function: FunctionRef,
        label: &str,
        session: &ExecutionSession<'_>,
    ) -> Result<ExecutionReport, VmError> {
        let mut executor = Executor::new(&self.runtime, module, function, &[])?;
        let return_value = executor.run()?;
        let failure = unsafe { self.runtime.result_failure_transport(&return_value) };
        let return_value = unsafe {
            // SAFETY: Executor transfers its live terminal value here before any callback or collection.
            self.runtime.gc().root_value_transport(return_value)
        }
        .ok_or_else(|| RuntimeError::module_validation("execution result root"))?;

        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: label.to_owned(),
            failure,
            return_value,
            jit: None,
            trace: session.trace(),
        })
    }

    /// Invokes a linked script implementation through a runtime-owned
    /// interface value. The method ID must belong to the value's retained definition
    /// prefix. The receiver and arguments remain rooted while the method body executes.
    /// Admit protected transport at the backend execution boundary.
    ///
    /// # Safety
    /// Keep all managed inputs live until the destination frame or result owner is
    /// published. Inputs that can be replaced through aliases require independent
    /// protection across callbacks. Root raw results before collection, reentry or
    /// source retirement; host capabilities must remain in their original host scope.
    /// Existing owner/type/generation checks cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn invoke_interface_method_transport(
        &self,
        interface: &Value,
        method: &DefinitionId,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        self.invoke_interface_method(interface, method, arguments)
    }

    pub(crate) fn invoke_interface_method(
        &self,
        interface: &Value,
        method: &DefinitionId,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let resolved = unsafe {
            self.runtime
                .resolve_interface_method_transport(interface, method)
        }
        .map_err(VmError::RuntimeError)?;
        let loaded = resolved.implementation(&self.runtime)?;
        let args = iter::once(*unsafe { resolved.receiver_transport() })
            .chain(arguments.iter().cloned())
            .collect::<Vec<_>>();
        let _argument_roots = unsafe {
            // SAFETY: No callbacks or safepoints separate receiver resolution, argument copying and checked-ID root publication.
            self.runtime
                .gc()
                .root_execution_values_transport(args.clone())
        }
        .ok_or(VmError::TypeMismatch("invalid interface method argument"))?;
        let _session = self.begin_execution(&loaded)?;
        self.runtime
            .validate_executable_module(&loaded)
            .map_err(VmError::RuntimeError)?;
        Executor::new_interface(&self.runtime, resolved, &args)?.run()
    }
}
