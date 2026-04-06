pub mod gc;
pub mod host;
pub mod reflection;
pub mod reload;
pub mod value;

use kagari_ir::bytecode::BytecodeModule;

use crate::{
    gc::{GcHeap, GcHeapConfig},
    host::{HostError, HostRegistry},
    reflection::ReflectionError,
    reload::{HotReloadCoordinator, ModuleEpoch},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeConfig {
    pub gc: GcHeapConfig,
}

#[derive(Debug)]
pub struct Runtime {
    gc: GcHeap,
    host: HostRegistry,
    reloads: HotReloadCoordinator,
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub name: String,
    pub epoch: ModuleEpoch,
    pub bytecode: BytecodeModule,
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            gc: GcHeap::new(config.gc),
            host: HostRegistry::default(),
            reloads: HotReloadCoordinator::default(),
        }
    }

    pub fn gc(&self) -> &GcHeap {
        &self.gc
    }

    pub fn host(&self) -> &HostRegistry {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut HostRegistry {
        &mut self.host
    }

    pub fn invoke_host(
        &self,
        symbol: &str,
        args: &[value::Value],
    ) -> Result<value::Value, HostError> {
        self.host.invoke(symbol, args)
    }

    pub fn reflect_type_of(&self, value: &value::Value) -> value::Value {
        reflection::type_of(value)
    }

    pub fn reflect_get_field(
        &self,
        value: &value::Value,
        field_name: &str,
    ) -> Result<value::Value, ReflectionError> {
        reflection::get_field(value, field_name)
    }

    pub fn reflect_set_field(
        &self,
        value: &value::Value,
        field_name: &str,
        next_value: value::Value,
    ) -> Result<value::Value, ReflectionError> {
        reflection::set_field(value, field_name, next_value)
    }

    pub fn reflect_set_index(
        &self,
        value: &value::Value,
        index: &value::Value,
        next_value: value::Value,
    ) -> Result<value::Value, ReflectionError> {
        reflection::set_index(value, index, next_value)
    }

    pub fn load_module(
        &mut self,
        name: impl Into<String>,
        bytecode: BytecodeModule,
    ) -> LoadedModule {
        let name = name.into();
        let epoch = self.reloads.publish(&name);
        LoadedModule {
            name,
            epoch,
            bytecode,
        }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }
}
