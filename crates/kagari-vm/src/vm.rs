use std::collections::HashMap;

use kagari_ir::bytecode::{BytecodeModule, FunctionRef};
use kagari_runtime::{LoadedModule, Runtime, value::Value};

use crate::error::VmError;
use crate::executor::Executor;

#[derive(Debug)]
pub struct Vm {
    runtime: Runtime,
    module_instances: HashMap<(String, u64), ModuleInstance>,
}

#[derive(Debug, Clone)]
pub(crate) enum ModuleState {
    Uninitialized,
    Initializing,
    Initialized,
    Failed(VmError),
}

#[derive(Debug, Clone)]
pub(crate) struct ModuleInstance {
    pub(crate) state: ModuleState,
    pub(crate) init_result: Value,
    pub(crate) module_slots: Vec<Value>,
}

impl ModuleInstance {
    pub(crate) fn is_initializing(&self) -> bool {
        matches!(self.state, ModuleState::Initializing)
    }
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
            module_instances: HashMap::new(),
        }
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    pub fn execute(
        &mut self,
        module: &LoadedModule,
        entry: &str,
    ) -> Result<ExecutionReport, VmError> {
        self.execute_module(module)?;
        let cache_key = (module.name.clone(), module.epoch.0);
        let module_instance = self
            .module_instances
            .get_mut(&cache_key)
            .expect("module instance should exist after module initialization");
        let entry_name = entry.to_owned();
        let entry = find_function_ref(&module.bytecode, &entry_name)
            .ok_or_else(|| VmError::MissingFunction(entry_name.clone()))?;
        let mut executor =
            Executor::new(&mut self.runtime, &module.bytecode, module_instance, entry)?;
        let return_value = executor.run()?;

        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: entry_name,
            return_value,
        })
    }

    pub fn execute_module(&mut self, module: &LoadedModule) -> Result<Value, VmError> {
        let cache_key = (module.name.clone(), module.epoch.0);
        if let Some(instance) = self.module_instances.get(&cache_key) {
            match &instance.state {
                ModuleState::Initialized => return Ok(instance.init_result.clone()),
                ModuleState::Initializing => return Ok(instance.init_result.clone()),
                ModuleState::Failed(error) => return Err(error.clone()),
                ModuleState::Uninitialized => {}
            }
        }

        let module_instance =
            self.module_instances
                .entry(cache_key)
                .or_insert_with(|| ModuleInstance {
                    state: ModuleState::Uninitialized,
                    init_result: Value::Unit,
                    module_slots: vec![Value::Unit; module.bytecode.module_slots.len()],
                });

        module_instance.state = ModuleState::Initializing;
        let result = match module.bytecode.module_init {
            Some(module_init) => {
                let mut executor = Executor::new(
                    &mut self.runtime,
                    &module.bytecode,
                    module_instance,
                    module_init,
                )?;
                executor.run()
            }
            None => Ok(Value::Unit),
        };

        match result {
            Ok(result) => {
                module_instance.state = ModuleState::Initialized;
                module_instance.init_result = result.clone();
                Ok(result)
            }
            Err(error) => {
                module_instance.state = ModuleState::Failed(error.clone());
                module_instance.init_result = Value::Unit;
                Err(error)
            }
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
