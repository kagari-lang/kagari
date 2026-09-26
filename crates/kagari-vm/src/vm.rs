use kagari_ir::bytecode::{BytecodeModule, FunctionRef};
use kagari_runtime::{
    BackendDiagnostic, BackendFunctionInput, BackendId, BackendInvocationError, CodegenBackend,
    ExecutionArtifactId, LoadedModule, ReloadDependencySnapshot, Runtime, value::Value,
};

use crate::debug::{DebugSession, SharedDebugSession};
use crate::error::VmError;
use crate::executor::Executor;
use std::{
    cell::{Ref, RefMut},
    rc::Rc,
};

#[derive(Debug)]
pub enum ReloadError {
    Validation(kagari_runtime::ReloadValidationError),
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
    pub jit: Option<JitExecutionReport>,
    pub trace: Option<kagari_runtime::ExecutionTrace>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JitExecutionReport {
    pub backend: BackendId,
    pub function: FunctionRef,
    pub status: JitExecutionStatus,
    pub artifact: Option<ExecutionArtifactId>,
    pub diagnostics: Vec<BackendDiagnostic>,
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
        program: kagari_ir::bytecode::BytecodeProgram,
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
        artifact: kagari_ir::bytecode::KbcArtifact,
        compatibility: &kagari_ir::bytecode::ArtifactCompatibility,
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
            .validate_debug_attach_boundary()
            .map_err(VmError::RuntimeError)?;
        self.debug_session = Some(Rc::new(SharedDebugSession(std::cell::RefCell::new(
            session,
        ))));
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

    fn begin_execution(
        &self,
        module: &LoadedModule,
    ) -> Result<kagari_runtime::ExecutionSession, VmError> {
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
            return_value,
            jit: None,
            trace: _session.trace(),
        })
    }

    /// Invokes a linked script implementation through a runtime-owned
    /// interface value. The receiver and arguments remain rooted while module
    /// the method body executes.
    pub fn invoke_interface_method(
        &mut self,
        interface: &Value,
        method: &kagari_common::identity::DefinitionId,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let resolved = self
            .runtime
            .resolve_interface_method(interface, method)
            .map_err(VmError::RuntimeError)?;
        let loaded = resolved.implementation().clone();
        let args = std::iter::once(resolved.receiver().clone())
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

    pub fn execute_with_backend<B: CodegenBackend>(
        &mut self,
        module: &LoadedModule,
        entry: &str,
        backend: &mut B,
    ) -> Result<ExecutionReport, VmError> {
        let _session = self.begin_execution(module)?;
        self.runtime
            .validate_loaded_module(module)
            .map_err(VmError::RuntimeError)?;
        let entry_name = entry.to_owned();
        let entry = find_function_ref(&module.bytecode, &entry_name)?;

        match self.try_execute_jit_entry(module, entry, backend)? {
            JitEntryResult::Native { value, report } => Ok(ExecutionReport {
                module_name: module.name.clone(),
                epoch: module.epoch.0,
                entry: entry_name,
                return_value: value,
                jit: Some(report),
                trace: _session.trace(),
            }),
            JitEntryResult::Fallback(report) => {
                let return_value = self.execute_interpreter_entry(module, entry)?;
                Ok(ExecutionReport {
                    module_name: module.name.clone(),
                    epoch: module.epoch.0,
                    entry: entry_name,
                    return_value,
                    jit: Some(report),
                    trace: _session.trace(),
                })
            }
        }
    }

    fn try_execute_jit_entry<B: CodegenBackend>(
        &self,
        module: &LoadedModule,
        entry: FunctionRef,
        backend: &mut B,
    ) -> Result<JitEntryResult, VmError> {
        let backend_id = backend.backend_id();
        let function = module
            .bytecode
            .functions
            .get(entry.index())
            .ok_or(VmError::InvalidFunctionRef(entry))?;
        if let Err(error) = self.runtime.validate_jit_boundary() {
            return Ok(JitEntryResult::Fallback(JitExecutionReport {
                backend: backend_id,
                function: entry,
                status: JitExecutionStatus::InterpreterFallback,
                artifact: None,
                diagnostics: vec![BackendDiagnostic::unsupported(format!(
                    "JIT disabled by runtime policy: {error}"
                ))],
            }));
        }
        let dependencies = ReloadDependencySnapshot::from_bytecode(&module.bytecode);
        let artifact = match backend
            .compile_function(BackendFunctionInput::new(module, entry).expect("resolved entry"))
        {
            Ok(artifact) => artifact,
            Err(error) if error.is_unsupported() => {
                return Ok(JitEntryResult::Fallback(JitExecutionReport {
                    backend: backend_id,
                    function: entry,
                    status: JitExecutionStatus::InterpreterFallback,
                    artifact: None,
                    diagnostics: error.diagnostics,
                }));
            }
            Err(error) => return Err(VmError::JitBackend(error.diagnostics)),
        };
        if let Some(report) = self.debug_fallback_report(&artifact, function, &backend_id) {
            return Ok(JitEntryResult::Fallback(report));
        }
        let artifact_id = self
            .runtime
            .register_executable_function_artifact(module.key(), dependencies, artifact.clone())
            .ok_or_else(|| {
                VmError::JitBackend(vec![BackendDiagnostic {
                    kind: kagari_runtime::BackendDiagnosticKind::InternalError,
                    message: format!(
                        "JIT artifact for `{}` could not be registered",
                        function.name
                    ),
                }])
            })?;
        let stack = self.runtime.enter_execution_stack(module)?;
        stack.push(module.slot(), entry, &[], None)?;
        match backend.invoke_function(&artifact, &self.runtime) {
            Ok(value) => Ok(JitEntryResult::Native {
                value,
                report: JitExecutionReport {
                    backend: backend_id,
                    function: entry,
                    status: JitExecutionStatus::Native,
                    artifact: Some(artifact_id),
                    diagnostics: Vec::new(),
                },
            }),
            Err(BackendInvocationError::UnsupportedArtifact(message)) => {
                Ok(JitEntryResult::Fallback(JitExecutionReport {
                    backend: backend_id,
                    function: entry,
                    status: JitExecutionStatus::InterpreterFallback,
                    artifact: Some(artifact_id),
                    diagnostics: vec![BackendDiagnostic::unsupported(message)],
                }))
            }
            Err(BackendInvocationError::RuntimeFailure(error)) => {
                Err(VmError::RuntimeError(error).with_trace(self.runtime.capture_error_trace()))
            }
            Err(error) => {
                Err(VmError::JitInvocation(error).with_trace(self.runtime.capture_error_trace()))
            }
        }
    }

    fn debug_fallback_report(
        &self,
        artifact: &kagari_runtime::ExecutableFunctionArtifact,
        function: &kagari_ir::bytecode::BytecodeFunction,
        backend_id: &BackendId,
    ) -> Option<JitExecutionReport> {
        self.debug_session.as_ref()?;
        let missing = artifact.debug.missing_requirements_for_function(function);
        if missing.is_empty() {
            return None;
        }
        Some(JitExecutionReport {
            backend: backend_id.clone(),
            function: function.id,
            status: JitExecutionStatus::InterpreterFallback,
            artifact: None,
            diagnostics: vec![BackendDiagnostic::unsupported(format!(
                "JIT fallback while debugging `{}`: missing {}",
                function.name,
                missing.join(", ")
            ))],
        })
    }

    fn execute_interpreter_entry(
        &mut self,
        module: &LoadedModule,
        entry: FunctionRef,
    ) -> Result<Value, VmError> {
        let mut executor = Executor::new(&self.runtime, module, entry, &[])?;
        executor.run()
    }
}

enum JitEntryResult {
    Native {
        value: Value,
        report: JitExecutionReport,
    },
    Fallback(JitExecutionReport),
}

fn find_function_ref(module: &BytecodeModule, name: &str) -> Result<FunctionRef, VmError> {
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
