use std::{
    cell::{RefCell, RefMut},
    collections::HashMap,
    ops::Deref,
    sync::Arc,
};

use kagari_ir::bytecode::{BytecodeModule, BytecodeProgram, ModuleRef};

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

/// A verified, linked module with immutable shared executable data.
///
/// ```compile_fail
/// fn mutate(module: &mut kagari_runtime::LoadedModule) {
///     module.bytecode.functions.clear();
/// }
/// ```
#[derive(Debug, Clone)]
pub struct LoadedModule {
    program: Arc<LinkedProgram>,
    slot: ModuleRef,
}

#[derive(Debug)]
struct LinkedProgram {
    root: ModuleRef,
    modules: Vec<LinkedModule>,
}
/// Immutable executable data, exposed only through a shared loaded handle.
#[derive(Debug)]
pub struct LinkedModule {
    pub id: ModuleId,
    pub name: String,
    pub epoch: ModuleEpoch,
    pub bytecode: BytecodeModule,
    registry_owner: crate::host::HostRegistryId,
    host_bindings: Vec<crate::host::HostFunctionId>,
}

impl Deref for LoadedModule {
    type Target = LinkedModule;
    fn deref(&self) -> &LinkedModule {
        &self.program.modules[self.slot.index()]
    }
}

impl LoadedModule {
    pub fn slot(&self) -> ModuleRef {
        self.slot
    }
    pub fn member(&self, slot: ModuleRef) -> Option<Self> {
        self.program.modules.get(slot.index())?;
        Some(Self {
            program: self.program.clone(),
            slot,
        })
    }
    pub fn member_data(&self, slot: ModuleRef) -> Option<&LinkedModule> {
        self.program.modules.get(slot.index())
    }
    pub fn members(&self) -> impl Iterator<Item = Self> + '_ {
        (0..self.program.modules.len()).map(|index| Self {
            program: self.program.clone(),
            slot: ModuleRef::new(index),
        })
    }
    pub fn program_root(&self) -> Self {
        Self {
            program: self.program.clone(),
            slot: self.program.root,
        }
    }
    fn program_key(&self) -> ModuleKey {
        self.program_root().key()
    }
    pub fn struct_layout(&self, id: kagari_ir::bytecode::StructId) -> Option<StructLayoutRef> {
        self.bytecode.structures.get(id.index())?;
        Some(StructLayoutRef {
            module: self.clone(),
            id,
        })
    }

    pub fn enum_variant(
        &self,
        id: kagari_ir::bytecode::EnumId,
        variant: u32,
    ) -> Option<EnumVariantRef> {
        self.bytecode
            .enumerations
            .get(id.index())?
            .variants
            .get(variant as usize)?;
        Some(EnumVariantRef {
            module: self.clone(),
            id,
            variant,
        })
    }
    pub fn host_binding(
        &self,
        import: kagari_ir::bytecode::HostImportId,
    ) -> Option<crate::host::HostFunctionId> {
        self.host_bindings.get(import.index()).copied()
    }

    pub(crate) fn belongs_to(&self, owner: crate::host::HostRegistryId) -> bool {
        self.registry_owner == owner
    }
    pub fn key(&self) -> ModuleKey {
        ModuleKey {
            id: self.id,
            epoch: self.epoch,
        }
    }
}

/// A verified layout that retains its immutable executable generation.
#[derive(Debug, Clone)]
pub struct EnumVariantRef {
    module: LoadedModule,
    id: kagari_ir::bytecode::EnumId,
    variant: u32,
}

impl EnumVariantRef {
    pub fn layout(&self) -> &kagari_ir::module::EnumLayout {
        &self.module.bytecode.enumerations[self.id.index()]
    }
    pub fn variant(&self) -> &kagari_ir::module::EnumVariantLayout {
        &self.layout().variants[self.variant as usize]
    }
    pub fn module(&self) -> &LoadedModule {
        &self.module
    }
}

impl PartialEq for EnumVariantRef {
    fn eq(&self, other: &Self) -> bool {
        self.module.registry_owner == other.module.registry_owner
            && self.layout() == other.layout()
            && self.variant == other.variant
    }
}

/// A verified layout that retains its immutable executable generation.
#[derive(Debug, Clone)]
pub struct StructLayoutRef {
    module: LoadedModule,
    id: kagari_ir::bytecode::StructId,
}

impl StructLayoutRef {
    pub fn layout(&self) -> &kagari_ir::module::StructLayout {
        &self.module.bytecode.structures[self.id.index()]
    }
    pub fn module(&self) -> &LoadedModule {
        &self.module
    }
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.module.registry_owner == other.module.registry_owner
            && ((Arc::ptr_eq(&self.module.program, &other.module.program)
                && self.module.slot == other.module.slot
                && self.id == other.id)
                || self.layout() == other.layout())
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

#[derive(Debug, Default, Clone)]
pub struct ModuleStore {
    inner: std::rc::Rc<RefCell<ModuleStoreInner>>,
}

#[derive(Debug, Default)]
struct ModuleStoreInner {
    next_id: usize,
    ids_by_member: HashMap<(String, kagari_common::identity::ModuleIdentity), ModuleId>,
    loaded: HashMap<ModuleKey, LoadedModule>,
    latest_by_name: HashMap<String, ModuleKey>,
    instances: HashMap<ModuleKey, ModuleInstance>,
    retentions: HashMap<ModuleKey, ModuleEpochRetentionCounts>,
}

impl ModuleStore {
    pub(crate) fn gc_roots(&self) -> Vec<Value> {
        self.inner
            .borrow()
            .instances
            .values()
            .flat_map(|instance| {
                instance
                    .module_slots
                    .iter()
                    .chain(instance.init_result.iter())
                    .cloned()
            })
            .collect()
    }
    pub(crate) fn load_program(
        &self,
        name: impl Into<String>,
        epoch: ModuleEpoch,
        bytecode: BytecodeProgram,
        registry_owner: crate::host::HostRegistryId,
        host_bindings: Vec<Vec<crate::host::HostFunctionId>>,
    ) -> LoadedModule {
        assert_eq!(
            bytecode.modules.len(),
            host_bindings.len(),
            "each program member must be linked"
        );
        let name = name.into();
        let mut inner = self.inner.borrow_mut();
        let root = bytecode.root;
        let modules = bytecode
            .modules
            .into_iter()
            .zip(host_bindings)
            .enumerate()
            .map(|(index, (bytecode, host_bindings))| {
                let identity = (name.clone(), bytecode.identity.clone());
                let id = if let Some(id) = inner.ids_by_member.get(&identity).copied() {
                    id
                } else {
                    let id = ModuleId::new(inner.next_id);
                    inner.next_id += 1;
                    inner.ids_by_member.insert(identity, id);
                    id
                };
                let display = if index == root.index() {
                    name.clone()
                } else {
                    format!("{}::{}", name, bytecode.identity)
                };
                LinkedModule {
                    id,
                    name: display,
                    epoch,
                    bytecode,
                    registry_owner,
                    host_bindings,
                }
            })
            .collect();
        let program = Arc::new(LinkedProgram { root, modules });
        let loaded = LoadedModule {
            program,
            slot: root,
        };
        for member in loaded.members() {
            let key = member.key();
            inner.instances.insert(key, ModuleInstance::new(&member));
            inner.retentions.entry(key).or_default();
            inner.loaded.insert(key, member);
        }
        inner.latest_by_name.insert(name, loaded.key());
        loaded
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
        let Some(module) = inner.loaded.get(&key) else {
            return false;
        };
        live_programs(&inner).contains(&module.program_key())
    }

    pub fn collect_unreachable_epochs(&self) -> Vec<ModuleKey> {
        let mut inner = self.inner.borrow_mut();
        let live = live_programs(&inner);
        let removable = inner
            .loaded
            .iter()
            .filter_map(|(key, module)| (!live.contains(&module.program_key())).then_some(*key))
            .collect::<Vec<_>>();
        for key in &removable {
            inner.loaded.remove(key);
            inner.instances.remove(key);
            inner.retentions.remove(key);
        }
        removable
    }
}

fn live_programs(inner: &ModuleStoreInner) -> std::collections::HashSet<ModuleKey> {
    inner
        .latest_by_name
        .values()
        .copied()
        .chain(
            inner
                .retentions
                .iter()
                .filter_map(|(key, counts)| counts.is_retained().then_some(*key)),
        )
        .filter_map(|key| inner.loaded.get(&key).map(LoadedModule::program_key))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_stable_module_ids_across_epochs() {
        let store = ModuleStore::default();
        let first = store.load_program(
            "game.player",
            ModuleEpoch(1),
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            crate::host::HostRegistryId::default(),
            vec![vec![]],
        );
        let second = store.load_program(
            "game.player",
            ModuleEpoch(2),
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            crate::host::HostRegistryId::default(),
            vec![vec![]],
        );
        let other = store.load_program(
            "game.world",
            ModuleEpoch(1),
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            crate::host::HostRegistryId::default(),
            vec![vec![]],
        );

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
        let module = store.load_program(
            "game.init",
            ModuleEpoch(1),
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            crate::host::HostRegistryId::default(),
            vec![vec![]],
        );

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
        let module = store.load_program(
            "game.init",
            ModuleEpoch(1),
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            crate::host::HostRegistryId::default(),
            vec![vec![]],
        );

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

        let next = store.load_program(
            "game.init",
            ModuleEpoch(2),
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            crate::host::HostRegistryId::default(),
            vec![vec![]],
        );
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
        let first = store.load_program(
            "game.player",
            ModuleEpoch(1),
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            crate::host::HostRegistryId::default(),
            vec![vec![]],
        );
        let second = store.load_program(
            "game.player",
            ModuleEpoch(2),
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            crate::host::HostRegistryId::default(),
            vec![vec![]],
        );

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
