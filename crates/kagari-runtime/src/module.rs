use std::{
    cell::{RefCell, RefMut},
    collections::HashMap,
};

use kagari_ir::bytecode::BytecodeModule;

use crate::{reload::ModuleEpoch, value::Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(u64);

impl ModuleId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleKey {
    pub id: ModuleId,
    pub epoch: ModuleEpoch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleEpochRetention {
    ActiveCall,
    RuntimeValue,
    CompiledArtifact,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModuleEpochRetentionCounts {
    pub active_calls: usize,
    pub runtime_values: usize,
    pub compiled_artifacts: usize,
}

impl ModuleEpochRetentionCounts {
    pub fn total(self) -> usize {
        self.active_calls + self.runtime_values + self.compiled_artifacts
    }

    pub fn is_retained(self) -> bool {
        self.total() > 0
    }

    fn increment(&mut self, retention: ModuleEpochRetention) {
        match retention {
            ModuleEpochRetention::ActiveCall => self.active_calls += 1,
            ModuleEpochRetention::RuntimeValue => self.runtime_values += 1,
            ModuleEpochRetention::CompiledArtifact => self.compiled_artifacts += 1,
        }
    }

    fn decrement(&mut self, retention: ModuleEpochRetention) -> bool {
        let counter = match retention {
            ModuleEpochRetention::ActiveCall => &mut self.active_calls,
            ModuleEpochRetention::RuntimeValue => &mut self.runtime_values,
            ModuleEpochRetention::CompiledArtifact => &mut self.compiled_artifacts,
        };
        if *counter == 0 {
            return false;
        }
        *counter -= 1;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleInitializationState {
    Uninitialized,
    Initializing,
    Initialized,
    Failed,
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub id: ModuleId,
    pub name: String,
    pub epoch: ModuleEpoch,
    pub bytecode: BytecodeModule,
}

impl LoadedModule {
    pub fn key(&self) -> ModuleKey {
        ModuleKey {
            id: self.id,
            epoch: self.epoch,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInstance {
    pub id: ModuleId,
    pub name: String,
    pub epoch: ModuleEpoch,
    pub state: ModuleInitializationState,
    pub init_result: Option<Value>,
    pub module_slots: Vec<Value>,
}

impl ModuleInstance {
    pub fn new(module: &LoadedModule) -> Self {
        Self {
            id: module.id,
            name: module.name.clone(),
            epoch: module.epoch,
            state: ModuleInitializationState::Uninitialized,
            init_result: None,
            module_slots: vec![Value::Unit; module.bytecode.module_slots.len()],
        }
    }

    pub fn is_initializing(&self) -> bool {
        matches!(self.state, ModuleInitializationState::Initializing)
    }

    pub fn begin_initialization(&mut self) {
        self.state = ModuleInitializationState::Initializing;
    }

    pub fn finish_initialization(&mut self, result: Value) {
        self.state = ModuleInitializationState::Initialized;
        self.init_result = Some(result);
    }

    pub fn fail_initialization(&mut self) {
        self.state = ModuleInitializationState::Failed;
        self.init_result = None;
    }
}

#[derive(Debug, Default)]
pub struct ModuleStore {
    inner: RefCell<ModuleStoreInner>,
}

#[derive(Debug, Default)]
struct ModuleStoreInner {
    next_id: usize,
    ids_by_name: HashMap<String, ModuleId>,
    loaded: HashMap<ModuleKey, LoadedModule>,
    latest_by_name: HashMap<String, ModuleKey>,
    instances: HashMap<ModuleKey, ModuleInstance>,
    retentions: HashMap<ModuleKey, ModuleEpochRetentionCounts>,
}

impl ModuleStore {
    pub fn load(
        &self,
        name: impl Into<String>,
        epoch: ModuleEpoch,
        bytecode: BytecodeModule,
    ) -> LoadedModule {
        let name = name.into();
        let mut inner = self.inner.borrow_mut();
        let id = if let Some(id) = inner.ids_by_name.get(&name).copied() {
            id
        } else {
            let id = ModuleId::new(inner.next_id);
            inner.next_id += 1;
            inner.ids_by_name.insert(name.clone(), id);
            id
        };

        let module = LoadedModule {
            id,
            name: name.clone(),
            epoch,
            bytecode,
        };
        let key = module.key();
        inner.latest_by_name.insert(name, key);
        inner.instances.insert(key, ModuleInstance::new(&module));
        inner.retentions.entry(key).or_default();
        inner.loaded.insert(key, module.clone());
        module
    }

    pub fn loaded(&self, key: ModuleKey) -> Option<LoadedModule> {
        self.inner.borrow().loaded.get(&key).cloned()
    }

    pub fn latest(&self, name: &str) -> Option<LoadedModule> {
        let inner = self.inner.borrow();
        let key = inner.latest_by_name.get(name)?;
        inner.loaded.get(key).cloned()
    }

    pub fn instance_snapshot(&self, key: ModuleKey) -> Option<ModuleInstance> {
        self.inner.borrow().instances.get(&key).cloned()
    }

    pub fn instance_mut(&self, key: ModuleKey) -> Option<RefMut<'_, ModuleInstance>> {
        RefMut::filter_map(self.inner.borrow_mut(), |inner| {
            inner.instances.get_mut(&key)
        })
        .ok()
    }

    pub fn loaded_count(&self) -> usize {
        self.inner.borrow().loaded.len()
    }

    pub fn retain_epoch(&self, key: ModuleKey, retention: ModuleEpochRetention) -> bool {
        let mut inner = self.inner.borrow_mut();
        if !inner.loaded.contains_key(&key) {
            return false;
        }
        inner
            .retentions
            .entry(key)
            .or_default()
            .increment(retention);
        true
    }

    pub fn release_epoch(&self, key: ModuleKey, retention: ModuleEpochRetention) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(counts) = inner.retentions.get_mut(&key) else {
            return false;
        };
        counts.decrement(retention)
    }

    pub fn retention_counts(&self, key: ModuleKey) -> ModuleEpochRetentionCounts {
        self.inner
            .borrow()
            .retentions
            .get(&key)
            .copied()
            .unwrap_or_default()
    }

    pub fn is_reachable(&self, key: ModuleKey) -> bool {
        let inner = self.inner.borrow();
        inner.latest_by_name.values().any(|latest| *latest == key)
            || inner
                .retentions
                .get(&key)
                .copied()
                .is_some_and(ModuleEpochRetentionCounts::is_retained)
    }

    pub fn collect_unreachable_epochs(&self) -> Vec<ModuleKey> {
        let mut inner = self.inner.borrow_mut();
        let latest_keys = inner.latest_by_name.values().copied().collect::<Vec<_>>();
        let removable = inner
            .loaded
            .keys()
            .copied()
            .filter(|key| {
                !latest_keys.contains(key)
                    && !inner
                        .retentions
                        .get(key)
                        .copied()
                        .is_some_and(ModuleEpochRetentionCounts::is_retained)
            })
            .collect::<Vec<_>>();

        for key in &removable {
            inner.loaded.remove(key);
            inner.instances.remove(key);
            inner.retentions.remove(key);
        }
        removable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_stable_module_ids_across_epochs() {
        let store = ModuleStore::default();
        let first = store.load("game.player", ModuleEpoch(1), BytecodeModule::default());
        let second = store.load("game.player", ModuleEpoch(2), BytecodeModule::default());
        let other = store.load("game.world", ModuleEpoch(1), BytecodeModule::default());

        assert_eq!(first.id, second.id);
        assert_ne!(first.id, other.id);
        assert_eq!(first.id.index(), 0);
        assert_eq!(other.id.index(), 1);
        assert_eq!(store.loaded_count(), 3);
        assert_eq!(store.latest("game.player").unwrap().epoch, ModuleEpoch(2));
    }

    #[test]
    fn creates_module_instances_with_explicit_initialization_state() {
        let store = ModuleStore::default();
        let module = store.load("game.init", ModuleEpoch(1), BytecodeModule::default());

        let instance = store.instance_snapshot(module.key()).unwrap();
        assert_eq!(instance.id, module.id);
        assert_eq!(instance.name, "game.init");
        assert_eq!(instance.epoch, ModuleEpoch(1));
        assert_eq!(instance.state, ModuleInitializationState::Uninitialized);
        assert_eq!(instance.init_result, None);
    }

    #[test]
    fn records_initialization_result_and_failure_state() {
        let store = ModuleStore::default();
        let module = store.load("game.init", ModuleEpoch(1), BytecodeModule::default());

        {
            let mut instance = store.instance_mut(module.key()).unwrap();
            instance.begin_initialization();
            assert!(instance.is_initializing());
            instance.finish_initialization(Value::I32(7));
        }
        assert_eq!(
            store.instance_snapshot(module.key()).unwrap().init_result,
            Some(Value::I32(7))
        );

        let next = store.load("game.init", ModuleEpoch(2), BytecodeModule::default());
        {
            let mut instance = store.instance_mut(next.key()).unwrap();
            instance.begin_initialization();
            instance.fail_initialization();
        }
        let failed = store.instance_snapshot(next.key()).unwrap();
        assert_eq!(failed.state, ModuleInitializationState::Failed);
        assert_eq!(failed.init_result, None);
    }

    #[test]
    fn keeps_latest_and_retained_old_epochs_reachable() {
        let store = ModuleStore::default();
        let first = store.load("game.player", ModuleEpoch(1), BytecodeModule::default());
        let second = store.load("game.player", ModuleEpoch(2), BytecodeModule::default());

        assert!(store.is_reachable(second.key()));
        assert!(!store.is_reachable(first.key()));
        assert!(store.retain_epoch(first.key(), ModuleEpochRetention::ActiveCall));
        assert!(store.retain_epoch(first.key(), ModuleEpochRetention::RuntimeValue));
        assert!(store.retain_epoch(first.key(), ModuleEpochRetention::CompiledArtifact));
        assert!(store.is_reachable(first.key()));
        assert_eq!(store.retention_counts(first.key()).active_calls, 1);
        assert_eq!(store.retention_counts(first.key()).runtime_values, 1);
        assert_eq!(store.retention_counts(first.key()).compiled_artifacts, 1);
        assert!(store.collect_unreachable_epochs().is_empty());
        assert!(store.loaded(first.key()).is_some());

        assert!(store.release_epoch(first.key(), ModuleEpochRetention::ActiveCall));
        assert!(store.release_epoch(first.key(), ModuleEpochRetention::RuntimeValue));
        assert!(store.release_epoch(first.key(), ModuleEpochRetention::CompiledArtifact));
        assert_eq!(store.collect_unreachable_epochs(), vec![first.key()]);
        assert!(store.loaded(first.key()).is_none());
        assert!(store.loaded(second.key()).is_some());
    }
}
