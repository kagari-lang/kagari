use kagari_common::identity::table::DefinitionId;
pub mod native;
use kagari_bytecode::{
    artifact::{ArtifactCompatibility, KbcArtifact},
    module::BytecodeModule,
    program::BytecodeProgram,
};
use kagari_common::identity::reference::DefinitionReference;
use kagari_runtime::{
    Runtime,
    error_trace::ResultFailure,
    module::LoadedModule,
    reload::ReloadValidationError,
    session::{ExecutionSession, ExecutionTrace},
    value::Value,
};
use std::{
    cell::{Ref, RefCell, RefMut},
    iter,
    rc::Rc,
};
use {
    kagari_abi::native::BackendId,
    kagari_contract::{ids::FunctionRef, native::ExecutableFunctionArtifact},
};

use crate::{
    debug::{DebugSession, SharedDebugSession},
    error::VmError,
    executor::Executor,
};

#[derive(Debug)]
pub enum ReloadError {
    Validation(ReloadValidationError),
}

#[derive(Debug)]
pub struct Vm {
    runtime: Runtime,
    debug_session: Option<Rc<SharedDebugSession>>,
}

#[derive(Debug)]
pub struct ExecutionReport {
    pub module_name: String,
    pub epoch: u64,
    pub entry: String,
    pub return_value: Value,
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
        Self {
            runtime,
            debug_session: None,
        }
    }

    pub fn reload_program(
        &mut self,
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
        &mut self,
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
        self.runtime
            .resources()
            .ensure_execution_allowed()
            .map_err(VmError::RuntimeError)?;
        self.debug_session = Some(Rc::new(SharedDebugSession(RefCell::new(session))));
        Ok(())
    }

    pub fn debug_session(&self) -> Option<Ref<'_, DebugSession>> {
        self.debug_session
            .as_ref()
            .map(|session| session.0.borrow())
    }

    pub fn debug_session_mut(&mut self) -> Option<RefMut<'_, DebugSession>> {
        self.debug_session
            .as_ref()
            .map(|session| session.0.borrow_mut())
    }

    fn begin_execution(&self, module: &LoadedModule) -> Result<ExecutionSession, VmError> {
        let session = self
            .runtime
            .begin_execution(module, self.runtime.execution_options())?;
        if let Some(debug) = &self.debug_session
            && self.runtime.attach_execution_observer(debug.clone())?
        {
            let mut debug = debug.0.borrow_mut();
            for member in session.root().members() {
                debug.resolve_module(&member, &self.runtime)?;
            }
        }
        Ok(session)
    }

    pub fn execute(
        &mut self,
        module: &LoadedModule,
        entry: &str,
    ) -> Result<ExecutionReport, VmError> {
        let _session = self.begin_execution(module)?;
        self.runtime
            .validate_loaded_module(module)
            .map_err(VmError::RuntimeError)?;
        let entry_name = entry.to_owned();
        let entry = find_function_ref(&module.bytecode, &entry_name)?;
        let mut executor = Executor::new(&self.runtime, module, entry, &[])?;
        let return_value = executor.run()?;

        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: entry_name,
            failure: self.runtime.result_failure(&return_value),
            return_value,
            jit: None,
            trace: _session.trace(),
        })
    }

    /// Invokes a linked script implementation through a runtime-owned
    /// interface value. The receiver and arguments remain rooted while module
    /// the method body executes.
    pub fn invoke_interface_method<I: DefinitionReference>(
        &mut self,
        interface: &Value,
        method: &I,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let resolved = self
            .runtime
            .resolve_interface_method(interface, method)
            .map_err(VmError::RuntimeError)?;
        let loaded = resolved.implementation().clone();
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
