use std::collections::HashMap;

use kagari_ir::bytecode::{
    BytecodeInstruction, BytecodeModule, CallTarget, FunctionRef, verify_module,
};
use kagari_runtime::{
    BackendDiagnostic, BackendFunctionInput, BackendId, BackendInvocationErrorKind, CodegenBackend,
    ExecutionArtifactId, LoadedModule, ModuleEpochRetention, ModuleInitializationState, ModuleKey,
    ModuleStore, ReloadDependencySnapshot, Runtime, value::Value,
};

use crate::debug::DebugSession;
use crate::error::VmError;
use crate::executor::Executor;

#[derive(Debug)]
pub struct Vm {
    runtime: Runtime,
    module_failures: HashMap<ModuleKey, VmError>,
    debug_session: Option<DebugSession>,
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
        self.debug_session = Some(session);
        Ok(())
    }

    pub fn debug_session(&self) -> Option<&DebugSession> {
        self.debug_session.as_ref()
    }

    pub fn debug_session_mut(&mut self) -> Option<&mut DebugSession> {
        self.debug_session.as_mut()
    }

    pub fn execute(
        &mut self,
        module: &LoadedModule,
        entry: &str,
    ) -> Result<ExecutionReport, VmError> {
        validate_executable_bytecode(&module.bytecode)?;
        self.execute_module(module)?;
        if let Some(debug_session) = self.debug_session.as_mut() {
            debug_session.resolve_module(
                module.id,
                &module.name,
                module.epoch.0,
                &module.bytecode,
                &self.runtime,
            )?;
        }
        let entry_name = entry.to_owned();
        let entry = find_function_ref(&module.bytecode, &entry_name)
            .ok_or_else(|| VmError::MissingFunction(entry_name.clone()))?;
        let _epoch_guard = ModuleEpochGuard::new(
            self.runtime.modules(),
            module.key(),
            ModuleEpochRetention::ActiveCall,
        );
        let module_instance = self
            .runtime
            .module_instance_mut(module)
            .expect("module instance should exist after module initialization");
        let mut executor = Executor::new(
            &self.runtime,
            &module.bytecode,
            module_instance,
            entry,
            self.debug_session.as_mut(),
        )?;
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
        validate_executable_bytecode(&module.bytecode)?;
        self.execute_module(module)?;
        if let Some(debug_session) = self.debug_session.as_mut() {
            debug_session.resolve_module(
                module.id,
                &module.name,
                module.epoch.0,
                &module.bytecode,
                &self.runtime,
            )?;
        }
        let entry_name = entry.to_owned();
        let entry = find_function_ref(&module.bytecode, &entry_name)
            .ok_or_else(|| VmError::MissingFunction(entry_name.clone()))?;

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
        validate_executable_bytecode(&module.bytecode)?;
        if let Some(debug_session) = self.debug_session.as_mut() {
            debug_session.resolve_module(
                module.id,
                &module.name,
                module.epoch.0,
                &module.bytecode,
                &self.runtime,
            )?;
        }
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

        {
            let mut module_instance = self
                .runtime
                .module_instance_mut(module)
                .expect("loaded module should have a runtime module instance");
            module_instance.begin_initialization();
        }

        let result = match module.bytecode.module_init {
            Some(module_init) => {
                let _epoch_guard = ModuleEpochGuard::new(
                    self.runtime.modules(),
                    module.key(),
                    ModuleEpochRetention::ActiveCall,
                );
                let module_instance = self
                    .runtime
                    .module_instance_mut(module)
                    .expect("loaded module should have a runtime module instance");
                let mut executor = Executor::new(
                    &self.runtime,
                    &module.bytecode,
                    module_instance,
                    module_init,
                    self.debug_session.as_mut(),
                );
                match executor {
                    Ok(ref mut executor) => executor.run(),
                    Err(error) => Err(error),
                }
            }
            None => Ok(Value::Unit),
        };

        match result {
            Ok(result) => {
                let mut module_instance = self
                    .runtime
                    .module_instance_mut(module)
                    .expect("loaded module should have a runtime module instance");
                module_instance.finish_initialization(result.clone());
                Ok(result)
            }
            Err(error) => {
                let mut module_instance = self
                    .runtime
                    .module_instance_mut(module)
                    .expect("loaded module should have a runtime module instance");
                module_instance.fail_initialization();
                self.module_failures.insert(key, error.clone());
                Err(error)
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
        let artifact = match backend.compile_function(BackendFunctionInput {
            module_key: module.key(),
            module_name: &module.name,
            module: &module.bytecode,
            function,
            dependencies: dependencies.clone(),
        }) {
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
        let _epoch_guard = ModuleEpochGuard::new(
            self.runtime.modules(),
            module.key(),
            ModuleEpochRetention::ActiveCall,
        );
        let _call_guard = RuntimeCallGuard::new(&self.runtime)?;
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
            Err(error) if error.kind == BackendInvocationErrorKind::UnsupportedArtifact => {
                Ok(JitEntryResult::Fallback(JitExecutionReport {
                    backend: backend_id,
                    function: entry,
                    status: JitExecutionStatus::InterpreterFallback,
                    artifact: Some(artifact_id),
                    diagnostics: vec![BackendDiagnostic::unsupported(error.message)],
                }))
            }
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
        let _epoch_guard = ModuleEpochGuard::new(
            self.runtime.modules(),
            module.key(),
            ModuleEpochRetention::ActiveCall,
        );
        let module_instance = self
            .runtime
            .module_instance_mut(module)
            .expect("module instance should exist after module initialization");
        let mut executor = Executor::new(
            &self.runtime,
            &module.bytecode,
            module_instance,
            entry,
            self.debug_session.as_mut(),
        )?;
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

struct RuntimeCallGuard<'a> {
    runtime: &'a Runtime,
    entered: bool,
}

impl<'a> RuntimeCallGuard<'a> {
    fn new(runtime: &'a Runtime) -> Result<Self, VmError> {
        runtime.enter_call().map_err(VmError::RuntimeError)?;
        Ok(Self {
            runtime,
            entered: true,
        })
    }
}

impl Drop for RuntimeCallGuard<'_> {
    fn drop(&mut self) {
        if self.entered {
            self.runtime.leave_call();
        }
    }
}

struct ModuleEpochGuard<'a> {
    modules: &'a ModuleStore,
    key: ModuleKey,
    retention: ModuleEpochRetention,
    retained: bool,
}

impl<'a> ModuleEpochGuard<'a> {
    fn new(modules: &'a ModuleStore, key: ModuleKey, retention: ModuleEpochRetention) -> Self {
        let retained = modules.retain_epoch(key, retention);
        Self {
            modules,
            key,
            retention,
            retained,
        }
    }
}

impl Drop for ModuleEpochGuard<'_> {
    fn drop(&mut self) {
        if self.retained {
            self.modules.release_epoch(self.key, self.retention);
        }
    }
}

fn find_function_ref(module: &BytecodeModule, name: &str) -> Option<FunctionRef> {
    module
        .functions
        .iter()
        .find(|function| function.name == name)
        .map(|function| function.id)
}

fn validate_executable_bytecode(module: &BytecodeModule) -> Result<(), VmError> {
    verify_module(module).map_err(VmError::BytecodeVerification)?;
    for function in &module.functions {
        for instruction in &function.instructions {
            let BytecodeInstruction::Call { callee, .. } = instruction else {
                continue;
            };
            match callee {
                CallTarget::Register(_) => {
                    return Err(VmError::UnsupportedCallTarget(callee.clone()));
                }
                CallTarget::Function(_)
                | CallTarget::BuiltinMethod(_)
                | CallTarget::RuntimeHelper(_) => {}
            }
        }
    }
    Ok(())
}
