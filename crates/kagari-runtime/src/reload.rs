use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleEpoch(pub u64);

#[derive(Debug, Default)]
pub struct HotReloadCoordinator {
    epochs: HashMap<String, ModuleEpoch>,
}

impl HotReloadCoordinator {
    pub fn publish(&mut self, module_name: &str) -> ModuleEpoch {
        let next = self
            .epochs
            .get(module_name)
            .map(|epoch| ModuleEpoch(epoch.0 + 1))
            .unwrap_or(ModuleEpoch(1));
        self.epochs.insert(module_name.to_string(), next);
        next
    }

    pub fn epoch_of(&self, module_name: &str) -> Option<ModuleEpoch> {
        self.epochs.get(module_name).copied()
    }
}
