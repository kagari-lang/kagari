use std::{
    cell::{RefCell, RefMut},
    collections::{HashMap, HashSet},
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

/// Verified executable code that can be linked independently into multiple runtimes.
/// Host bindings and module instances are created separately for each runtime.
#[derive(Debug, Clone)]
pub struct VerifiedProgram {
    root: ModuleRef,
    modules: Arc<[Arc<BytecodeModule>]>,
    dependencies: crate::cache::ReloadDependencySnapshot,
}

impl VerifiedProgram {
    pub fn new(program: BytecodeProgram) -> Result<Self, crate::RuntimeError> {
        kagari_ir::bytecode::verify_program(&program).map_err(|error| {
            crate::RuntimeError::module_validation(format!("bytecode validation failed: {error}"))
        })?;
        Ok(Self::from_verified(program))
    }

    fn from_verified(program: BytecodeProgram) -> Self {
        let dependencies = crate::cache::ReloadDependencySnapshot::from_program(&program);
        Self {
            root: program.root,
            modules: program.modules.into_iter().map(Arc::new).collect(),
            dependencies,
        }
    }

    pub fn root(&self) -> ModuleRef {
        self.root
    }

    pub fn modules(&self) -> &[Arc<BytecodeModule>] {
        &self.modules
    }

    pub(crate) fn dependencies(&self) -> &crate::cache::ReloadDependencySnapshot {
        &self.dependencies
    }
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
    pub bytecode: Arc<BytecodeModule>,
    registry_owner: crate::host::HostRegistryId,
    pub(crate) host_bindings: LinkedHostBindings,
}

#[derive(Debug, Default)]
pub(crate) struct LinkedHostBindings {
    pub functions: Vec<crate::host::HostFunctionId>,
    pub paths: Vec<crate::host::HostPathDescriptorId>,
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
        self.host_bindings.functions.get(import.index()).copied()
    }

    pub fn path_binding(
        &self,
        path: kagari_ir::bytecode::PathId,
    ) -> Option<crate::host::HostPathDescriptorId> {
        self.host_bindings.paths.get(path.index()).copied()
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

#[derive(Debug, Clone)]
pub struct ModuleStore {
    resources: std::rc::Rc<crate::ResourceState>,
    inner: std::rc::Rc<RefCell<ModuleStoreInner>>,
}

impl Default for ModuleStore {
    fn default() -> Self {
        Self::new(std::rc::Rc::new(crate::ResourceState::default()))
    }
}

#[derive(Debug, Default)]
struct ModuleStoreInner {
    next_id: usize,
    ids_by_member: HashMap<(String, kagari_common::identity::ModuleIdentity), ModuleId>,
    loaded: HashMap<ModuleKey, LoadedModule>,
    latest_by_name: HashMap<String, ModuleKey>,
    staged: HashSet<ModuleKey>,
    instances: HashMap<ModuleKey, ModuleInstance>,
    retentions: HashMap<ModuleKey, ModuleEpochRetentionCounts>,
}

/// Owns an unpublished program and its isolated module instances.
/// Dropping the candidate releases those instances without touching active entries.
#[derive(Debug)]
pub(crate) struct StagedProgram {
    store: ModuleStore,
    module: LoadedModule,
}

impl StagedProgram {
    pub(crate) fn module(&self) -> &LoadedModule {
        &self.module
    }

    pub(crate) fn publish(self) -> LoadedModule {
        let mut inner = self.store.inner.borrow_mut();
        inner.staged.remove(&self.module.program_key());
        inner
            .latest_by_name
            .insert(self.module.name.clone(), self.module.key());
        self.module.clone()
    }
}

impl Drop for StagedProgram {
    fn drop(&mut self) {
        let mut inner = self.store.inner.borrow_mut();
        if inner.staged.remove(&self.module.program_key()) {
            for member in self.module.members() {
                let key = member.key();
                inner.loaded.remove(&key);
                inner.instances.remove(&key);
                inner.retentions.remove(&key);
            }
            self.store
                .resources
                .release_modules(self.module.members().count());
        }
    }
}

impl ModuleStore {
    pub(crate) fn new(resources: std::rc::Rc<crate::ResourceState>) -> Self {
        Self {
            resources,
            inner: Default::default(),
        }
    }

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
    pub(crate) fn stage_program(
        &self,
        name: impl Into<String>,
        epoch: ModuleEpoch,
        bytecode: BytecodeProgram,
        registry_owner: crate::host::HostRegistryId,
        host_bindings: Vec<LinkedHostBindings>,
    ) -> Result<StagedProgram, crate::RuntimeError> {
        self.stage_verified_program(
            name,
            epoch,
            VerifiedProgram::from_verified(bytecode),
            registry_owner,
            host_bindings,
        )
    }

    pub(crate) fn stage_verified_program(
        &self,
        name: impl Into<String>,
        epoch: ModuleEpoch,
        program: VerifiedProgram,
        registry_owner: crate::host::HostRegistryId,
        host_bindings: Vec<LinkedHostBindings>,
    ) -> Result<StagedProgram, crate::RuntimeError> {
        assert_eq!(
            program.modules.len(),
            host_bindings.len(),
            "each program member must be linked"
        );
        let name = name.into();
        let mut inner = self.inner.borrow_mut();
        self.resources.admit_modules(program.modules.len())?;
        let root = program.root;
        let modules = program
            .modules
            .iter()
            .cloned()
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
        inner.staged.insert(loaded.program_key());
        Ok(StagedProgram {
            store: self.clone(),
            module: loaded,
        })
    }
    pub fn loaded(&self, key: ModuleKey) -> Option<LoadedModule> {
        self.inner.borrow().loaded.get(&key).cloned()
    }

    pub fn latest(&self, name: &str) -> Option<LoadedModule> {
        let inner = self.inner.borrow();
        let key = inner.latest_by_name.get(name)?;
        inner.loaded.get(key).cloned()
    }

    pub(crate) fn allows_instance_access(&self, key: ModuleKey) -> bool {
        self.resources.active_session().is_none_or(|session| {
            session.options.phase != crate::ExecutionPhase::CandidateInitialization
                || session.root.members().any(|member| member.key() == key)
        })
    }

    pub fn instance_snapshot(&self, key: ModuleKey) -> Option<ModuleInstance> {
        if !self.allows_instance_access(key) {
            return None;
        }
        self.inner.borrow().instances.get(&key).cloned()
    }

    pub fn instance_mut(&self, key: ModuleKey) -> Option<RefMut<'_, ModuleInstance>> {
        if !self.allows_instance_access(key) {
            return None;
        }
        self.instance_mut_for_cleanup(key)
    }

    pub(crate) fn instance_mut_for_cleanup(
        &self,
        key: ModuleKey,
    ) -> Option<RefMut<'_, ModuleInstance>> {
        RefMut::filter_map(self.inner.borrow_mut(), |inner| {
            inner.instances.get_mut(&key)
        })
        .ok()
    }

    pub(crate) fn is_staged(&self, module: &LoadedModule) -> bool {
        self.inner.borrow().staged.contains(&module.program_key())
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
        self.resources.release_modules(removable.len());
        removable
    }
}

fn live_programs(inner: &ModuleStoreInner) -> std::collections::HashSet<ModuleKey> {
    inner
        .latest_by_name
        .values()
        .copied()
        .chain(inner.staged.iter().copied())
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

    use crate::{Runtime, RuntimeConfig};

    #[test]
    fn invalid_code_cannot_become_a_shared_verified_program() {
        let invalid = BytecodeProgram {
            root: ModuleRef::new(1),
            modules: vec![BytecodeModule::default()],
        };
        assert!(VerifiedProgram::new(invalid).is_err());
    }

    #[test]
    fn verified_code_is_shared_but_runtime_linkage_and_instances_are_private() {
        let code = VerifiedProgram::new(BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        })
        .unwrap();
        let mut first_runtime = Runtime::new(RuntimeConfig::default());
        let mut second_runtime = Runtime::new(RuntimeConfig::default());
        let first = first_runtime
            .load_verified_program("shared", code.clone())
            .unwrap();
        let second = second_runtime
            .load_verified_program("shared", code)
            .unwrap();

        assert!(Arc::ptr_eq(&first.bytecode, &second.bytecode));
        assert!(first_runtime.validate_loaded_module(&first).is_ok());
        assert!(second_runtime.validate_loaded_module(&second).is_ok());
        assert!(first_runtime.validate_loaded_module(&second).is_err());
        assert!(second_runtime.validate_loaded_module(&first).is_err());
        first_runtime
            .modules()
            .instance_mut(first.key())
            .unwrap()
            .finish_initialization(Value::I32(7));
        assert_eq!(
            first_runtime
                .modules()
                .instance_snapshot(first.key())
                .unwrap()
                .init_result,
            Some(Value::I32(7))
        );
        assert_eq!(
            second_runtime
                .modules()
                .instance_snapshot(second.key())
                .unwrap()
                .init_result,
            None
        );
    }

    #[test]
    fn staged_programs_keep_instances_alive_without_activating_them() {
        let store = ModuleStore::default();
        let stage = |epoch| {
            let dependency = BytecodeModule::default();
            let mut root = BytecodeModule::default();
            root.identity.path.push("root".into());
            root.dependencies.push(ModuleRef::new(0));
            store
                .stage_program(
                    "game.player",
                    ModuleEpoch(epoch),
                    BytecodeProgram {
                        root: ModuleRef::new(1),
                        modules: vec![dependency, root],
                    },
                    crate::host::HostRegistryId::default(),
                    vec![LinkedHostBindings::default(), LinkedHostBindings::default()],
                )
                .unwrap()
        };
        let baseline = stage(1).publish();
        let abandoned = stage(2);
        let candidate = stage(3);
        let abandoned_keys = abandoned
            .module
            .members()
            .map(|m| m.key())
            .collect::<Vec<_>>();
        for member in candidate.module.members() {
            store
                .instance_mut(member.key())
                .unwrap()
                .finish_initialization(Value::I32(73));
            assert!(store.is_reachable(member.key()));
        }
        assert_eq!(store.latest("game.player").unwrap().key(), baseline.key());
        assert_eq!(store.loaded_count(), 6);
        assert_eq!(store.resources.counters().loaded_modules, 6);
        assert!(store.collect_unreachable_epochs().is_empty());
        assert_eq!(store.gc_roots(), vec![Value::I32(73); 2]);

        drop(abandoned);
        for key in abandoned_keys {
            assert!(store.loaded(key).is_none());
            assert!(store.instance_snapshot(key).is_none());
        }
        assert_eq!(store.loaded_count(), 4);
        assert_eq!(store.resources.counters().loaded_modules, 4);
        assert_eq!(store.latest("game.player").unwrap().key(), baseline.key());
        assert!(store.collect_unreachable_epochs().is_empty());

        let published = candidate.publish();
        assert_eq!(store.latest("game.player").unwrap().key(), published.key());
        for member in published.members() {
            let instance = store.instance_snapshot(member.key()).unwrap();
            assert_eq!(instance.state, ModuleInitializationState::Initialized);
            assert_eq!(instance.init_result, Some(Value::I32(73)));
        }
        assert_eq!(store.collect_unreachable_epochs().len(), 2);
        assert_eq!(store.loaded_count(), 2);
        assert_eq!(store.resources.counters().loaded_modules, 2);
        assert_eq!(store.gc_roots(), vec![Value::I32(73); 2]);
    }

    #[test]
    fn abandoning_candidates_releases_admission_even_after_quarantine() {
        let resources = std::rc::Rc::new(crate::ResourceState::new(crate::ResourcePolicy {
            max_modules: Some(1),
            ..Default::default()
        }));
        let store = ModuleStore::new(resources.clone());
        let stage = |epoch| {
            store.stage_program(
                "candidate",
                ModuleEpoch(epoch),
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
        };
        let candidate = stage(1).unwrap();
        assert_eq!(
            stage(2).unwrap_err().kind(),
            crate::RuntimeErrorKind::ResourceLimitExceeded
        );
        assert_eq!(store.loaded_count(), 1);
        assert_eq!(resources.counters().loaded_modules, 1);
        assert!(store.latest("candidate").is_none());
        drop(candidate);
        assert_eq!(resources.counters().loaded_modules, 0);
        let candidate = stage(3).unwrap();
        resources.quarantine("test failed initializer invariant");
        drop(candidate);
        assert_eq!(store.loaded_count(), 0);
        assert_eq!(resources.counters().loaded_modules, 0);
        assert!(resources.is_quarantined());
    }

    #[test]
    fn assigns_stable_module_ids_across_epochs() {
        let store = ModuleStore::default();
        let first = store
            .stage_program(
                "game.player",
                ModuleEpoch(1),
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish();
        let second = store
            .stage_program(
                "game.player",
                ModuleEpoch(2),
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish();
        let other = store
            .stage_program(
                "game.world",
                ModuleEpoch(1),
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish();

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
        let module = store
            .stage_program(
                "game.init",
                ModuleEpoch(1),
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish();

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
        let module = store
            .stage_program(
                "game.init",
                ModuleEpoch(1),
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish();

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

        let next = store
            .stage_program(
                "game.init",
                ModuleEpoch(2),
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish();
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
        let first = store
            .stage_program(
                "game.player",
                ModuleEpoch(1),
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish();
        let second = store
            .stage_program(
                "game.player",
                ModuleEpoch(2),
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish();

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
