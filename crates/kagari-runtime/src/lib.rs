pub mod gc;
pub mod host;
pub mod reload;
pub mod value;

use kagari_ir::module::IrModule;

use crate::{
    gc::{GcHeap, GcHeapConfig},
    host::HostRegistry,
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
    pub ir: IrModule,
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

    pub fn load_module(&mut self, name: impl Into<String>, ir: IrModule) -> LoadedModule {
        let name = name.into();
        let epoch = self.reloads.publish(&name);
        LoadedModule { name, epoch, ir }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }
}
