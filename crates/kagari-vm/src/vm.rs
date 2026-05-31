use std::collections::HashMap;

use kagari_ir::bytecode::{
    BytecodeInstruction, BytecodeModule, CallTarget, FunctionRef, verify_module,
};
use kagari_runtime::{
    LoadedModule, ModuleEpochRetention, ModuleInitializationState, ModuleKey, ModuleStore, Runtime,
    value::Value,
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

    pub fn attach_debug_session(&mut self, session: DebugSession) {
        self.debug_session = Some(session);
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
            debug_session.resolve_module(module.id, &module.name, module.epoch.0, &module.bytecode);
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
        })
    }

    pub fn execute_module(&mut self, module: &LoadedModule) -> Result<Value, VmError> {
        validate_executable_bytecode(&module.bytecode)?;
        if let Some(debug_session) = self.debug_session.as_mut() {
            debug_session.resolve_module(module.id, &module.name, module.epoch.0, &module.bytecode);
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
