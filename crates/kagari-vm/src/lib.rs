use kagari_runtime::{LoadedModule, Runtime, value::Value};

#[derive(Debug)]
pub struct Vm {
    runtime: Runtime,
}

#[derive(Debug)]
pub struct ExecutionReport {
    pub module_name: String,
    pub epoch: u64,
    pub entry: String,
    pub return_value: Value,
}

#[derive(Debug)]
pub enum VmError {
    MissingFunction(String),
}

impl Vm {
    pub fn new(runtime: Runtime) -> Self {
        Self { runtime }
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
        if module
            .ir
            .functions
            .iter()
            .all(|function| function.name != entry)
        {
            return Err(VmError::MissingFunction(entry.to_string()));
        }

        Ok(ExecutionReport {
            module_name: module.name.clone(),
            epoch: module.epoch.0,
            entry: entry.to_string(),
            return_value: Value::Unit,
        })
    }
}
