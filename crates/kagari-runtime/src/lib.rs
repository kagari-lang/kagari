pub mod builtin;
pub mod gc;
pub mod host;
pub mod module;
pub mod reflection;
pub mod reload;
pub mod value;

use kagari_ir::bytecode::{BuiltinMethod, BytecodeModule};

pub use module::{
    LoadedModule, ModuleId, ModuleInitializationState, ModuleInstance, ModuleKey, ModuleStore,
};

use crate::{
    builtin::BuiltinError,
    gc::{GcHeap, GcHeapConfig, GcRootId, HeapObjectId},
    host::{HostError, HostRegistry},
    reflection::ReflectionError,
    reload::HotReloadCoordinator,
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
    modules: ModuleStore,
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            gc: GcHeap::new(config.gc),
            host: HostRegistry::default(),
            reloads: HotReloadCoordinator::default(),
            modules: ModuleStore::default(),
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

    pub fn modules(&self) -> &ModuleStore {
        &self.modules
    }

    pub fn module_instance_snapshot(&self, module: &LoadedModule) -> Option<ModuleInstance> {
        self.modules.instance_snapshot(module.key())
    }

    pub fn module_instance_mut(
        &self,
        module: &LoadedModule,
    ) -> Option<std::cell::RefMut<'_, ModuleInstance>> {
        self.modules.instance_mut(module.key())
    }

    pub fn root_value(&self, value: value::Value) -> Option<GcRootId> {
        self.gc.root_value(value)
    }

    pub fn root_snapshot(&self, id: GcRootId) -> Option<value::Value> {
        self.gc.root_snapshot(id)
    }

    pub fn update_root(&self, id: GcRootId, value: value::Value) -> Option<()> {
        self.gc.update_root(id, value)
    }

    pub fn release_root(&self, id: GcRootId) -> Option<value::Value> {
        self.gc.release_root(id)
    }

    pub fn trace_roots(&self) -> Vec<HeapObjectId> {
        self.gc.trace_roots()
    }

    pub fn invoke_host(
        &self,
        symbol: &str,
        args: &[value::Value],
    ) -> Result<value::Value, HostError> {
        self.host.invoke(symbol, args)
    }

    pub fn reflect_type_of(&self, value: &value::Value) -> value::Value {
        reflection::type_of(&self.gc, value)
    }

    pub fn reflect_get_field(
        &self,
        value: &value::Value,
        field_name: &str,
    ) -> Result<value::Value, ReflectionError> {
        reflection::get_field(&self.gc, value, field_name)
    }

    pub fn reflect_set_field(
        &self,
        value: &value::Value,
        field_name: &str,
        next_value: value::Value,
    ) -> Result<value::Value, ReflectionError> {
        reflection::set_field(&self.gc, value, field_name, next_value)
    }

    pub fn reflect_set_index(
        &self,
        value: &value::Value,
        index: &value::Value,
        next_value: value::Value,
    ) -> Result<value::Value, ReflectionError> {
        reflection::set_index(&self.gc, value, index, next_value)
    }

    pub fn invoke_builtin(
        &self,
        method: BuiltinMethod,
        args: &[value::Value],
    ) -> Result<value::Value, BuiltinError> {
        builtin::invoke(&self.gc, method, args)
    }

    pub fn load_module(
        &mut self,
        name: impl Into<String>,
        bytecode: BytecodeModule,
    ) -> LoadedModule {
        let name = name.into();
        let epoch = self.reloads.publish(&name);
        self.modules.load(name, epoch, bytecode)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }
}
