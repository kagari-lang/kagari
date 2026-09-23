use std::collections::HashMap;

use kagari_ir::bytecode::{BytecodeInstruction, BytecodeModule, CallTarget, FunctionRef};
use kagari_runtime::{
    BackendDiagnostic, BackendFunctionInput, BackendId, BackendInvocationError, CodegenBackend,
    ExecutionArtifactId, LoadedModule, ModuleInitializationState, ModuleKey,
    ReloadDependencySnapshot, Runtime, value::Value,
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
    Initialization(VmError),
}

#[derive(Debug)]
pub struct Vm {
    runtime: Runtime,
    module_failures: HashMap<ModuleKey, VmError>,
    debug_session: Option<Rc<SharedDebugSession>>,
}

#[derive(Debug)]
pub struct ExecutionReport {
    pub module_name: String,
    pub epoch: u64,
    pub entry: String,
    pub return_value: Value,
    pub jit: Option<JitExecutionReport>,
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
            module_failures: HashMap::new(),
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
        self.initialize_and_publish(candidate)
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
        self.initialize_and_publish(candidate)
    }

    fn initialize_and_publish(
        &mut self,
        candidate: kagari_runtime::StagedReload,
    ) -> Result<LoadedModule, ReloadError> {
        let session = self
            .runtime
            .begin_candidate_initialization(&candidate)
            .map_err(|error| ReloadError::Initialization(VmError::RuntimeError(error)))?;
        let result = self.execute_module(candidate.module());
        drop(session);
        let result = result.and_then(|value| match candidate.initialization_error() {
            Some(error) => Err(VmError::RuntimeError(error)),
            None => Ok(value),
        });
        if let Err(error) = result {
            for member in candidate.module().members() {
                self.module_failures.remove(&member.key());
            }
            return Err(ReloadError::Initialization(error));
        }
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
                debug.resolve_module(
                    member.id,
                    &member.name,
                    member.epoch.0,
                    &member.bytecode,
                    &self.runtime,
                )?;
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
        validate_executable_bytecode(&module.bytecode)?;
        let entry_name = entry.to_owned();
        let entry = find_function_ref(&module.bytecode, &entry_name)?;
        self.execute_module(module)?;
        let mut executor = Executor::new(&self.runtime, module, entry, &[])?;
        let return_value = executor.run()?;

        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: entry_name,
            return_value,
            jit: None,
        })
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
        validate_executable_bytecode(&module.bytecode)?;
        let entry_name = entry.to_owned();
        let entry = find_function_ref(&module.bytecode, &entry_name)?;
        self.execute_module(module)?;

        match self.try_execute_jit_entry(module, entry, backend)? {
            JitEntryResult::Native { value, report } => Ok(ExecutionReport {
                module_name: module.name.clone(),
                epoch: module.epoch.0,
                entry: entry_name,
                return_value: value,
                jit: Some(report),
            }),
            JitEntryResult::Fallback(report) => {
                let return_value = self.execute_interpreter_entry(module, entry)?;
                Ok(ExecutionReport {
                    module_name: module.name.clone(),
                    epoch: module.epoch.0,
                    entry: entry_name,
                    return_value,
                    jit: Some(report),
                })
            }
        }
    }

    pub fn execute_module(&mut self, module: &LoadedModule) -> Result<Value, VmError> {
        let _session = self.begin_execution(module)?;
        self.runtime
            .validate_loaded_module(module)
            .map_err(VmError::RuntimeError)?;
        if let Some(instance) = self.runtime.module_instance_snapshot(module) {
            match instance.state {
                ModuleInitializationState::Initializing => {
                    return Err(VmError::ModuleInitializing(module.key()));
                }
                ModuleInitializationState::Failed => {
                    return Err(self
                        .module_failures
                        .get(&module.key())
                        .cloned()
                        .unwrap_or(VmError::UnsupportedInstruction("module_init_failed")));
                }
                ModuleInitializationState::Initialized
                | ModuleInitializationState::Uninitialized => {}
            }
        }
        let mut reachable = std::collections::HashSet::new();
        let mut pending = vec![module.slot()];
        while let Some(slot) = pending.pop() {
            if reachable.insert(slot) {
                pending.extend_from_slice(
                    &module
                        .member_data(slot)
                        .expect("verified module slot")
                        .bytecode
                        .dependencies,
                );
            }
        }
        for member in module
            .members()
            .filter(|member| reachable.contains(&member.slot()))
        {
            if let Err(error) = self.initialize_one(&member) {
                self.runtime
                    .fail_module_initialization(module)
                    .map_err(VmError::RuntimeError)?;
                self.module_failures.insert(module.key(), error.clone());
                return Err(error);
            }
        }
        Ok(self
            .runtime
            .module_instance_snapshot(module)
            .expect("initialized module")
            .init_result
            .unwrap_or(Value::Unit))
    }

    fn initialize_one(&mut self, module: &LoadedModule) -> Result<Value, VmError> {
        self.runtime
            .validate_loaded_module(module)
            .map_err(VmError::RuntimeError)?;
        validate_executable_bytecode(&module.bytecode)?;
        let key = module.key();
        if let Some(instance) = self.runtime.module_instance_snapshot(module) {
            match instance.state {
                ModuleInitializationState::Initialized => {
                    return Ok(instance.init_result.unwrap_or(Value::Unit));
                }
                ModuleInitializationState::Initializing => {
                    return Err(VmError::ModuleInitializing(key));
                }
                ModuleInitializationState::Failed => {
                    return Err(self
                        .module_failures
                        .get(&key)
                        .cloned()
                        .unwrap_or(VmError::UnsupportedInstruction("module_init_failed")));
                }
                ModuleInitializationState::Uninitialized => {}
            }
        }

        let initialization = self
            .runtime
            .begin_module_initialization(module)
            .map_err(VmError::RuntimeError)?;

        let result = match module.bytecode.module_init {
            Some(module_init) => {
                let mut executor = Executor::new(&self.runtime, module, module_init, &[]);
                match executor {
                    Ok(ref mut executor) => executor.run(),
                    Err(error) => Err(error),
                }
            }
            None => Ok(Value::Unit),
        };

        let result = match result {
            Ok(value) => initialization.finish(value).map_err(VmError::RuntimeError),
            Err(error) => {
                drop(initialization);
                Err(error)
            }
        };
        if let Err(error) = &result {
            self.module_failures.insert(key, error.clone());
        }
        result
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
            Err(BackendInvocationError::RuntimeFailure(error)) => Err(VmError::RuntimeError(error)),
            Err(error) => Err(VmError::JitInvocation(error)),
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

fn validate_executable_bytecode(module: &BytecodeModule) -> Result<(), VmError> {
    for function in &module.functions {
        for instruction in &function.instructions {
            let BytecodeInstruction::Call { callee, .. } = instruction else {
                continue;
            };
            match callee {
                CallTarget::Register(_) => {
                    return Err(VmError::UnsupportedCallTarget(callee.clone()));
                }
                CallTarget::HostFunction(_)
                | CallTarget::Function(_)
                | CallTarget::ModuleFunction { .. }
                | CallTarget::StandardIntrinsic(_)
                | CallTarget::RuntimeHelper(_) => {}
            }
        }
    }
    Ok(())
}
