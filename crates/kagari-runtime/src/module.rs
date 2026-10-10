pub(crate) mod collection;
mod constants;
mod descriptor_index;
pub(crate) mod descriptors;
pub mod execution;
mod layout_identity;
mod layout_scope;
mod layouts;
mod records;
pub mod retention;
pub(crate) mod staging;
mod state;
use crate::{
    cache::ReloadDependencySnapshot,
    error::RuntimeError,
    frame::types::bindings::TypeBindings,
    host::{HostFunctionId, HostPathDescriptorId, HostRegistryId},
    metadata::TypeId,
    module::{
        execution::ExecutionModule,
        layout_identity::{LayoutIdentity, ProgramLayouts},
        records::ModuleRecord,
        retention::{ProgramLease, Retentions},
        staging::StagedProgram,
    },
    native::binding::LinkedNativeFunction,
    reload::{ModuleEpoch, ModuleEpochAllocator},
    value::Value,
};
use kagari_bytecode::{
    artifact::{ArtifactFingerprint, ArtifactValidationError},
    instruction::{EnumId, PathId, StructId},
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram},
};

use kagari_contract::layout::{EnumLayout, EnumVariantLayout, StructLayout};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath, ModuleIdentity,
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
        table::{DefinitionId, DefinitionTable, DefinitionView},
    },
};
use std::{
    cell::{BorrowError, RefCell},
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::{Arc, Weak},
};

#[cfg(test)]
use std::cell::RefMut;

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
    program: Arc<ProgramDescriptor>,
    slot: ModuleRef,
}

/// Verified executable code that can be linked independently into multiple runtimes.
/// Host bindings and module instances are created separately for each runtime.
#[derive(Debug, Clone)]
pub struct VerifiedProgram {
    root: ModuleRef,
    modules: Arc<[Arc<BytecodeModule<DefinitionId>>]>,
    definitions: DefinitionTable,
    execution: Arc<[ExecutionModule]>,
    version: Arc<[Arc<BytecodeModule<DefinitionId>>]>,
    version_definitions: DefinitionTable,
    dependencies: ReloadDependencySnapshot,
}

impl VerifiedProgram {
    pub fn new(program: BytecodeProgram) -> Result<Self, RuntimeError> {
        let program = VerifiedBytecodeProgram::new(program).map_err(|error| match error {
            ArtifactValidationError::ResourceLimit(_) => {
                RuntimeError::resource_limit(error.to_string())
            }
            ArtifactValidationError::Bytecode(error) => {
                RuntimeError::module_validation(format!("bytecode validation failed: {error}"))
            }
            error => RuntimeError::module_validation(error.to_string()),
        })?;
        Ok(Self::from_bytecode(program))
    }

    /// Adopt bytecode-owned immutable verification evidence. Runtime-local host,
    /// native, ownership and generation checks still run when linking this code.
    pub fn from_bytecode(program: VerifiedBytecodeProgram) -> Self {
        let dependencies = ReloadDependencySnapshot::from_program(
            &program
                .to_unverified(&CancellationToken::default())
                .expect("verified program retains its bounded identity scope"),
        );
        let definitions = program.definitions().clone();
        let modules: Arc<[Arc<BytecodeModule<DefinitionId>>]> = program
            .program()
            .modules
            .iter()
            .cloned()
            .map(Arc::new)
            .collect();
        let mut allocation_work = 0;
        let mut execution: Vec<ExecutionModule> = modules
            .iter()
            .enumerate()
            .map(|(index, module)| {
                ExecutionModule::prepare(
                    module,
                    program
                        .suspensions(ModuleRef::new(index))
                        .expect("verified module suspension facts"),
                    &mut allocation_work,
                )
            })
            .collect();
        let layouts = execution
            .iter()
            .map(|module| {
                module
                    .functions
                    .iter()
                    .map(|function| function.registers.clone())
                    .collect()
            })
            .collect::<Vec<Vec<_>>>();
        for (index, prepared) in execution.iter_mut().enumerate() {
            prepared.prepare_calls(ModuleRef::new(index), &modules[index], &layouts);
        }
        Self {
            execution: execution.into(),
            root: program.program().root,
            version: modules.clone(),
            version_definitions: definitions.clone(),
            definitions,
            modules,
            dependencies,
        }
    }

    pub(crate) fn paths<T: DefinitionRecord<DefinitionId>>(
        &self,
        record: &T,
    ) -> Result<T::Rebind<DefinitionPath>, RuntimeError> {
        record
            .map_identities(&mut DefinitionMapper::new(
                &mut |id| Ok(self.definitions.resolve(*id)?.to_path()),
                &CancellationToken::default(),
            ))
            .map_err(|error| RuntimeError::module_validation(error.to_string()))
    }

    pub fn root(&self) -> ModuleRef {
        self.root
    }

    pub fn modules(&self) -> &[Arc<BytecodeModule<DefinitionId>>] {
        &self.modules
    }

    /// Identity of the immutable verified program, shared by clones and loads.
    /// Equal bytecode or a matching compatibility hash does not imply this identity.
    pub fn same_version(&self, other: &Self) -> bool {
        self.root == other.root && Arc::ptr_eq(&self.version, &other.version)
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    /// Import exact identities once while retaining the immutable source version.
    /// Mapping changes only identity representation; no verification flag or hash
    /// can create this evidence, and the original sealed records stay immutable.
    pub(crate) fn normalized(&self, context: &DefinitionContext) -> Result<Self, RuntimeError> {
        if self.definitions.id() == context.snapshot().id() {
            return Ok(self.clone());
        }
        let cancel = CancellationToken::default();
        let mut ids = HashSet::new();
        for module in self.modules.iter() {
            module
                .visit_definitions(
                    &mut |id| {
                        ids.insert(*id);
                        Ok(())
                    },
                    &cancel,
                )
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        }
        if ids.is_empty() {
            return Ok(self.clone());
        }
        let remap = context
            .import(&self.definitions, ids)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let modules = self
            .modules
            .iter()
            .map(|module| {
                module
                    .map_identities(&mut DefinitionMapper::new(
                        &mut |id| remap.map(*id).map_err(DefinitionMappingError::from),
                        &cancel,
                    ))
                    .map(Arc::new)
            })
            .collect::<Result<_, _>>()
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        Ok(Self {
            root: self.root,
            modules,
            definitions: context.snapshot(),
            execution: self.execution.clone(),
            version: self.version.clone(),
            version_definitions: self.version_definitions.clone(),
            dependencies: self.dependencies.clone(),
        })
    }

    pub(crate) fn dependencies(&self) -> &ReloadDependencySnapshot {
        &self.dependencies
    }
}

#[derive(Debug)]
struct ProgramDescriptor {
    code: VerifiedProgram,
    layouts: ProgramLayouts,
    root: ModuleRef,
    fingerprint: ArtifactFingerprint,
    modules: Vec<LinkedModule>,
}

/// Immutable executable data, exposed only through a shared loaded handle.
#[derive(Debug)]
pub struct LinkedModule {
    pub id: ModuleId,
    pub name: String,
    pub epoch: ModuleEpoch,
    pub bytecode: Arc<BytecodeModule<DefinitionId>>,
    registry_owner: HostRegistryId,
    host_types: HashMap<DefinitionId, TypeId>,
    pub(crate) host_functions: Vec<HostFunctionId>,
    pub(crate) host_paths: Vec<HostPathDescriptorId>,
}

#[derive(Debug, Default)]
pub(crate) struct LinkedHostBindings {
    pub types: HashMap<DefinitionId, TypeId>,
    pub functions: Vec<HostFunctionId>,
    pub native: Vec<Arc<LinkedNativeFunction>>,
    pub paths: Vec<HostPathDescriptorId>,
}

impl Deref for LoadedModule {
    type Target = LinkedModule;

    fn deref(&self) -> &LinkedModule {
        &self.program.modules[self.slot.index()]
    }
}

impl LoadedModule {
    pub(crate) fn execution(&self) -> &ExecutionModule {
        &self.program.code.execution[self.slot.index()]
    }

    pub(crate) fn host_type(&self, id: DefinitionId) -> Option<TypeId> {
        self.host_types.get(&id).copied()
    }

    /// Materialize editable authoring metadata without verification evidence.
    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<BytecodeModule, RuntimeError> {
        self.bytecode
            .map_identities(&mut DefinitionMapper::new(
                &mut |id| Ok(self.definitions().resolve(*id)?.to_path()),
                cancel,
            ))
            .map_err(|error| RuntimeError::module_validation(error.to_string()))
    }

    pub(crate) fn definition(&self, id: DefinitionId) -> Result<DefinitionView<'_>, RuntimeError> {
        self.definitions()
            .resolve(id)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))
    }

    pub(crate) fn definition_name(&self, id: DefinitionId) -> Option<&str> {
        self.definitions()
            .resolve(id)
            .ok()?
            .segments()
            .last()
            .map(|part| part.name)
    }

    pub fn definitions(&self) -> &DefinitionTable {
        self.program.code.definitions()
    }

    pub fn verified_program(&self) -> &VerifiedProgram {
        &self.program.code
    }

    pub fn program_fingerprint(&self) -> ArtifactFingerprint {
        self.program.fingerprint
    }

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

    pub(crate) fn program_identity(&self) -> (HostRegistryId, ModuleKey) {
        (self.registry_owner, self.program_key())
    }

    fn program_key(&self) -> ModuleKey {
        self.program_root().key()
    }

    pub fn struct_layout(&self, id: StructId) -> Option<StructLayoutRef> {
        let layout = self.bytecode.structures.get(id.index())?;
        self.applied_struct_layout(id, &layout.arguments)
    }

    pub fn enum_variant(&self, id: EnumId, variant: u32) -> Option<EnumVariantRef> {
        let layout = self.bytecode.enumerations.get(id.index())?;
        self.applied_enum_variant(id, &layout.arguments, variant)
    }

    pub fn path_binding(&self, path: PathId) -> Option<HostPathDescriptorId> {
        self.host_paths.get(path.index()).copied()
    }

    pub(crate) fn belongs_to(&self, owner: HostRegistryId) -> bool {
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
    id: EnumId,
    variant: u32,
    applied: Option<Arc<EnumLayout<DefinitionId>>>,
    canonical: Option<LayoutIdentity>,
    pub(crate) environment: Option<Arc<TypeBindings>>,
}

impl EnumVariantRef {
    pub(crate) fn registry_owner(&self) -> HostRegistryId {
        self.module.registry_owner
    }

    pub fn layout(&self) -> &EnumLayout<DefinitionId> {
        self.applied
            .as_deref()
            .unwrap_or(&self.module.bytecode.enumerations[self.id.index()])
    }

    pub fn variant(&self) -> &EnumVariantLayout<DefinitionId> {
        &self.layout().variants[self.variant as usize]
    }

    pub fn module(&self) -> &LoadedModule {
        &self.module
    }
}

impl PartialEq for EnumVariantRef {
    fn eq(&self, other: &Self) -> bool {
        self.module.registry_owner == other.module.registry_owner
            && self.layout().declaration == other.layout().declaration
            && self.layout().arguments == other.layout().arguments
            && self.variant().declaration == other.variant().declaration
    }
}

/// A verified layout that retains its immutable executable generation.
#[derive(Debug, Clone)]
pub struct StructLayoutRef {
    module: LoadedModule,
    id: StructId,
    applied: Option<Arc<StructLayout<DefinitionId>>>,
    canonical: Option<LayoutIdentity>,
    pub(crate) environment: Option<Arc<TypeBindings>>,
}

impl StructLayoutRef {
    /// Clones of one immutable layout can be checked without walking its type
    /// graph, including layouts carrying a generic lexical environment.
    fn same_instance(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.module.program, &other.module.program)
            && self.canonical.is_some()
            && self.canonical == other.canonical
            && self.environment.is_none()
            && other.environment.is_none()
        {
            return true;
        }
        self.module.registry_owner == other.module.registry_owner
            && Arc::ptr_eq(&self.module.program, &other.module.program)
            && self.module.slot == other.module.slot
            && self.id == other.id
            && match (&self.applied, &other.applied) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
            && match (&self.environment, &other.environment) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }

    pub fn layout(&self) -> &StructLayout<DefinitionId> {
        self.applied
            .as_deref()
            .unwrap_or(&self.module.bytecode.structures[self.id.index()])
    }

    pub fn module(&self) -> &LoadedModule {
        &self.module
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.same_instance(other)
            || (self.module.registry_owner == other.module.registry_owner
                && self.matches_type(
                    &other.type_expression(),
                    &other.module,
                    other.environment.as_deref(),
                ))
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInstance {
    pub id: ModuleId,
    pub name: String,
    pub epoch: ModuleEpoch,
    pub module_slots: Vec<Value>,
}

impl ModuleInstance {
    pub fn new(module: &LoadedModule) -> Self {
        Self {
            id: module.id,
            name: module.name.clone(),
            epoch: module.epoch,
            // Empty storage has no outgoing edges. Checked slot writes publish values.
            module_slots: vec![Value::Unit; module.bytecode.module_slots.len()],
        }
    }
}

#[derive(Debug, Default)]
pub struct ModuleStore {
    inner: RefCell<ModuleStoreInner>,
}

#[derive(Debug, Default)]
struct ModuleStoreInner {
    epochs: ModuleEpochAllocator,
    next_id: usize,
    ids_by_member: HashMap<(String, ModuleIdentity), ModuleId>,
    records: HashMap<ModuleKey, ModuleRecord>,
    latest_by_name: HashMap<String, ModuleKey>,
    staged: HashMap<ModuleKey, Weak<()>>,
    retentions: HashMap<ModuleKey, Retentions>,
}

impl ModuleStoreInner {
    fn available(&self, key: ModuleKey) -> Option<&LoadedModule> {
        let module = &self.records.get(&key)?.module;
        self.staged
            .get(&module.program_key())
            .is_none_or(|lease| lease.strong_count() != 0)
            .then_some(module)
    }
}

impl ModuleStore {
    pub(crate) fn reserve_epoch(&self, name: &str) -> Result<ModuleEpoch, RuntimeError> {
        self.inner
            .try_borrow_mut()
            .map_err(|_| {
                RuntimeError::module_validation(
                    "module store is borrowed during version reservation",
                )
            })?
            .epochs
            .reserve(name)
    }

    pub(crate) fn retain_program(
        &self,
        module: &LoadedModule,
        kind: ModuleEpochRetention,
    ) -> Option<ProgramLease> {
        let mut inner = self.inner.try_borrow_mut().ok()?;
        if !module
            .members()
            .all(|member| inner.resolve(&member).is_some())
        {
            return None;
        }
        let lease = ProgramLease::new(kind);
        for member in module.members() {
            inner
                .retentions
                .get_mut(&member.key())?
                .register(&lease)
                .ok()?;
        }
        Some(lease)
    }

    pub(crate) fn stage_verified_program(
        &self,
        name: impl Into<String>,
        epoch: ModuleEpoch,
        program: VerifiedProgram,
        registry_owner: HostRegistryId,
        host_bindings: Vec<LinkedHostBindings>,
    ) -> Result<StagedProgram, RuntimeError> {
        assert_eq!(
            program.modules.len(),
            host_bindings.len(),
            "each program member must be linked"
        );
        let name = name.into();
        let mut inner = self.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed during staging")
        })?;
        inner
            .records
            .len()
            .checked_add(program.modules.len())
            .ok_or_else(|| RuntimeError::resource_limit("loaded modules"))?;
        let root = program.root;
        let fingerprint = program.dependencies.module_fingerprint;
        let mut native_links = Vec::with_capacity(host_bindings.len());
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
                let LinkedHostBindings {
                    types,
                    functions,
                    native,
                    paths,
                } = host_bindings;
                native_links.push(native);
                LinkedModule {
                    id,
                    name: display,
                    epoch,
                    bytecode,
                    registry_owner,
                    host_types: types,
                    host_functions: functions,
                    host_paths: paths,
                }
            })
            .collect();
        let program = Arc::new(ProgramDescriptor {
            layouts: ProgramLayouts::prepare(&program.modules),
            code: program,
            root,
            fingerprint,
            modules,
        });
        let loaded = LoadedModule {
            program,
            slot: root,
        };
        for (member, native) in loaded.members().zip(native_links) {
            let key = member.key();
            inner.retentions.entry(key).or_default();
            inner.records.insert(key, ModuleRecord::new(member, native));
        }
        let lease = Arc::new(());
        inner
            .staged
            .insert(loaded.program_key(), Arc::downgrade(&lease));
        Ok(StagedProgram {
            module: loaded,
            lease,
        })
    }

    pub fn loaded(&self, key: ModuleKey) -> Option<LoadedModule> {
        self.inner.borrow().available(key).cloned()
    }

    pub(crate) fn contains_module(&self, module: &LoadedModule) -> Result<bool, BorrowError> {
        Ok(self.inner.try_borrow()?.resolve(module).is_some())
    }

    pub fn latest(&self, name: &str) -> Option<LoadedModule> {
        let inner = self.inner.borrow();
        let key = inner.latest_by_name.get(name)?;
        inner.available(*key).cloned()
    }

    pub(crate) fn instance_snapshot(&self, key: ModuleKey) -> Option<ModuleInstance> {
        let inner = self.inner.try_borrow().ok()?;
        inner.available(key)?;
        inner
            .records
            .get(&key)
            .map(|record| record.instance.clone())
    }

    #[cfg(test)]
    pub(crate) fn instance_mut(&self, key: ModuleKey) -> Option<RefMut<'_, ModuleInstance>> {
        RefMut::filter_map(self.inner.try_borrow_mut().ok()?, |inner| {
            inner.available(key)?;
            inner
                .records
                .get_mut(&key)
                .map(|record| &mut record.instance)
        })
        .ok()
    }

    pub(crate) fn is_staged(&self, module: &LoadedModule) -> bool {
        self.inner
            .borrow()
            .staged
            .get(&module.program_key())
            .is_some_and(|lease| lease.strong_count() != 0)
    }

    pub(crate) fn has_abandoned_programs(&self) -> Result<bool, RuntimeError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| {
                RuntimeError::module_validation(
                    "module store is borrowed at a collection safepoint",
                )
            })?
            .staged
            .values()
            .any(|lease| lease.strong_count() == 0))
    }

    /// Installed members available for access, excluding abandoned candidates.
    pub fn loaded_count(&self) -> usize {
        let inner = self.inner.borrow();
        inner
            .records
            .keys()
            .filter(|key| inner.available(**key).is_some())
            .count()
    }

    pub(crate) fn retain_module(
        &self,
        module: &LoadedModule,
        kind: ModuleEpochRetention,
    ) -> Option<ProgramLease> {
        let mut inner = self.inner.try_borrow_mut().ok()?;
        inner.resolve(module)?;
        let lease = ProgramLease::new(kind);
        inner
            .retentions
            .get_mut(&module.key())?
            .register(&lease)
            .ok()?;
        Some(lease)
    }

    pub fn retention_counts(&self, key: ModuleKey) -> ModuleEpochRetentionCounts {
        let inner = self.inner.borrow();
        inner
            .available(key)
            .and_then(|_| inner.retentions.get(&key))
            .map(Retentions::counts)
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn is_program_root(&self, key: ModuleKey) -> bool {
        let inner = self.inner.borrow();
        let Some(module) = inner.available(key) else {
            return false;
        };
        root_programs(&inner).contains(&module.program_key())
    }
}

fn root_programs(inner: &ModuleStoreInner) -> HashSet<ModuleKey> {
    inner
        .latest_by_name
        .values()
        .copied()
        .chain(inner.staged.keys().copied())
        .chain(
            inner
                .retentions
                .iter()
                .filter_map(|(key, counts)| counts.is_retained().then_some(*key)),
        )
        .filter_map(|key| inner.available(key).map(LoadedModule::program_key))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::resource::ResourceState;
    use crate::{Runtime, RuntimeConfig, error::RuntimeErrorKind, gc::GcHeap};
    use kagari_bytecode::program::ModuleRef;
    use kagari_bytecode::{
        instruction::{ConstantId, ConstantOperand},
        module::BytecodeModuleSlot,
    };

    use super::*;

    fn collect(store: &ModuleStore) -> Vec<ModuleKey> {
        GcHeap::new(Default::default(), ResourceState::default())
            .collect(store, &[])
            .unwrap()
            .reclaimed_modules
    }

    fn slot_values(store: &ModuleStore) -> Vec<Value> {
        let graph = store.collection_graph().unwrap();
        let mut values = Vec::new();
        for key in graph.roots() {
            graph
                .trace(key, &mut |value| values.push(*value), &mut Vec::new())
                .unwrap();
        }
        values
    }

    #[test]
    fn invalid_code_cannot_become_a_shared_verified_program() {
        let invalid = BytecodeProgram {
            root: ModuleRef::new(1),
            modules: vec![BytecodeModule::default()],
        };
        assert!(VerifiedProgram::new(invalid).is_err());
    }

    #[test]
    fn shared_verification_rejects_in_memory_resource_exhaustion() {
        let oversized = BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default(); 1_025],
        };
        assert_eq!(
            VerifiedProgram::new(oversized).unwrap_err().kind(),
            RuntimeErrorKind::ResourceLimitExceeded
        );
    }

    #[test]
    fn verified_code_is_shared_but_runtime_linkage_and_instances_are_private() {
        let code = VerifiedProgram::new(BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                constants: vec![
                    ConstantOperand::Str("shared".into()),
                    ConstantOperand::Str("pool-only".into()),
                ],
                module_slots: vec![BytecodeModuleSlot {
                    name: "state".into(),
                    ty: kagari_abi::representation::ValueType::I32,
                    mutable: true,
                }],
                ..Default::default()
            }],
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
        let shared = ConstantId::new(0);
        let escaped = first_runtime.read_constant(&first, shared).unwrap();
        let before = first_runtime.gc().stats();
        for _ in 0..100 {
            assert_eq!(
                first_runtime.read_constant(&first, shared).unwrap(),
                escaped
            );
        }
        assert!(first_runtime.read_constant(&second, shared).is_err());
        assert!(
            first_runtime
                .read_constant(&first, ConstantId::new(2))
                .is_err()
        );
        assert_eq!(first_runtime.gc().stats(), before);
        let second_value = second_runtime.read_constant(&second, shared).unwrap();
        assert_ne!(escaped, second_value);
        for (runtime, value) in [(&first_runtime, escaped), (&second_runtime, second_value)] {
            let Value::Str(id) = value else {
                panic!("expected string")
            };
            assert_eq!(&*runtime.gc().string(id).unwrap(), "shared");
        }
        first_runtime
            .modules()
            .instance_mut(first.key())
            .unwrap()
            .module_slots[0] = Value::I32(7);
        assert_eq!(
            first_runtime
                .modules()
                .instance_snapshot(first.key())
                .unwrap()
                .module_slots,
            vec![Value::I32(7)]
        );
        assert_eq!(
            second_runtime
                .modules()
                .instance_snapshot(second.key())
                .unwrap()
                .module_slots,
            vec![Value::Unit]
        );

        let pool_only = first_runtime
            .read_constant(&first, ConstantId::new(1))
            .unwrap();
        assert_eq!(
            first_runtime.collect_garbage().unwrap().reclaimed_objects,
            0
        );
        let escaped_root = first_runtime.root_value(escaped).unwrap();
        let mut replacement = first.to_unverified(&CancellationToken::default()).unwrap();
        replacement.constants.clear();
        let candidate = first_runtime
            .stage_reload_program(
                &first,
                "shared",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![replacement],
                },
            )
            .unwrap();
        first_runtime.publish_staged_reload(candidate).unwrap();
        let collected = first_runtime.collect_garbage().unwrap();
        assert_eq!(collected.reclaimed_modules, vec![first.key()]);
        assert_eq!(collected.reclaimed_objects, 1);
        assert!(!first_runtime.gc().validate_value(&pool_only));
        assert!(first_runtime.gc().validate_value(&escaped));
        assert!(first_runtime.read_constant(&first, shared).is_err());
        drop(escaped_root);
        assert_eq!(
            first_runtime.collect_garbage().unwrap().reclaimed_objects,
            1
        );
        assert!(!first_runtime.gc().validate_value(&escaped));
        assert_eq!(
            second_runtime.collect_garbage().unwrap().reclaimed_objects,
            0
        );
        assert_eq!(
            second_runtime.read_constant(&second, shared).unwrap(),
            second_value
        );
    }

    #[test]
    fn staged_programs_keep_instances_alive_without_activating_them() {
        let store = ModuleStore::default();
        let stage = |epoch| {
            let dependency = BytecodeModule {
                module_slots: vec![BytecodeModuleSlot {
                    name: "state".into(),
                    ty: kagari_abi::representation::ValueType::I32,
                    mutable: true,
                }],
                ..Default::default()
            };
            let mut root = BytecodeModule {
                module_slots: dependency.module_slots.clone(),
                ..Default::default()
            };
            root.identity.path.push("root".into());
            root.dependencies.push(ModuleRef::new(0));
            store
                .stage_verified_program(
                    "game.player",
                    ModuleEpoch(epoch),
                    VerifiedProgram::new(BytecodeProgram {
                        root: ModuleRef::new(1),
                        modules: vec![dependency, root],
                    })
                    .unwrap(),
                    crate::host::HostRegistryId::default(),
                    vec![LinkedHostBindings::default(), LinkedHostBindings::default()],
                )
                .unwrap()
        };
        let baseline = stage(1).publish(&store).unwrap();
        let abandoned = stage(2);
        let candidate = stage(3);
        let abandoned_keys = abandoned
            .module
            .members()
            .map(|m| m.key())
            .collect::<Vec<_>>();
        for member in candidate.module.members() {
            store.instance_mut(member.key()).unwrap().module_slots[0] = Value::I32(73);
            assert!(store.is_program_root(member.key()));
        }
        assert_eq!(store.latest("game.player").unwrap().key(), baseline.key());
        assert_eq!(store.loaded_count(), 6);
        assert!(collect(&store).is_empty());
        assert_eq!(
            slot_values(&store)
                .into_iter()
                .filter(|value| *value == Value::I32(73))
                .count(),
            2
        );

        drop(abandoned);
        for key in &abandoned_keys {
            assert!(store.loaded(*key).is_none());
            assert!(store.instance_snapshot(*key).is_none());
        }
        assert_eq!(store.loaded_count(), 4);
        assert_eq!(store.latest("game.player").unwrap().key(), baseline.key());
        assert_eq!(
            collect(&store).into_iter().collect::<HashSet<_>>(),
            abandoned_keys.into_iter().collect()
        );

        let published = candidate.publish(&store).unwrap();
        assert_eq!(store.latest("game.player").unwrap().key(), published.key());
        for member in published.members() {
            let instance = store.instance_snapshot(member.key()).unwrap();
            assert_eq!(instance.module_slots, vec![Value::I32(73)]);
        }
        assert_eq!(collect(&store).len(), 2);
        assert_eq!(store.loaded_count(), 2);
        assert_eq!(
            slot_values(&store)
                .into_iter()
                .filter(|value| *value == Value::I32(73))
                .count(),
            2
        );
    }

    #[test]
    fn abandoning_candidates_invalidates_access_even_after_quarantine() {
        let runtime = Runtime::default();
        let store = runtime.modules();
        let stage = |epoch| {
            store.stage_verified_program(
                "candidate",
                ModuleEpoch(epoch),
                VerifiedProgram::new(BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                })
                .unwrap(),
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
        };
        let candidate = stage(1).unwrap();
        assert_eq!(store.loaded_count(), 1);
        assert!(store.latest("candidate").is_none());
        drop(candidate);
        assert_eq!(store.loaded_count(), 0);
        let candidate = stage(3).unwrap();
        runtime
            .resources()
            .quarantine("test failed initializer invariant");
        drop(candidate);
        assert_eq!(store.loaded_count(), 0);
        assert!(runtime.is_quarantined());
    }

    #[test]
    fn assigns_stable_module_ids_across_epochs() {
        let store = ModuleStore::default();
        let first = store
            .stage_verified_program(
                "game.player",
                ModuleEpoch(1),
                VerifiedProgram::new(BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                })
                .unwrap(),
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish(&store)
            .unwrap();
        let second = store
            .stage_verified_program(
                "game.player",
                ModuleEpoch(2),
                VerifiedProgram::new(BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                })
                .unwrap(),
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish(&store)
            .unwrap();
        let other = store
            .stage_verified_program(
                "game.world",
                ModuleEpoch(1),
                VerifiedProgram::new(BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                })
                .unwrap(),
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish(&store)
            .unwrap();

        assert_eq!(first.id, second.id);
        assert_ne!(first.id, other.id);
        assert_eq!(first.id.index(), 0);
        assert_eq!(other.id.index(), 1);
        assert_eq!(store.loaded_count(), 3);
        assert_eq!(store.latest("game.player").unwrap().epoch, ModuleEpoch(2));
    }

    #[test]
    fn snapshot_during_mutable_instance_borrow_does_not_panic() {
        let store = ModuleStore::default();
        let module = store
            .stage_verified_program(
                "game.snapshot",
                ModuleEpoch(1),
                VerifiedProgram::new(BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                })
                .unwrap(),
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish(&store)
            .unwrap();
        let held = store.instance_mut(module.key()).unwrap();
        assert!(store.instance_snapshot(module.key()).is_none());
        drop(held);
        assert!(store.instance_snapshot(module.key()).is_some());
    }

    #[test]
    fn keeps_latest_and_retained_old_epochs_reachable() {
        let store = ModuleStore::default();
        let first = store
            .stage_verified_program(
                "game.player",
                ModuleEpoch(1),
                VerifiedProgram::new(BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                })
                .unwrap(),
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish(&store)
            .unwrap();
        let second = store
            .stage_verified_program(
                "game.player",
                ModuleEpoch(2),
                VerifiedProgram::new(BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                })
                .unwrap(),
                crate::host::HostRegistryId::default(),
                vec![LinkedHostBindings::default()],
            )
            .unwrap()
            .publish(&store)
            .unwrap();

        assert!(store.is_program_root(second.key()));
        assert!(!store.is_program_root(first.key()));
        let call = store
            .retain_module(&first, ModuleEpochRetention::ActiveCall)
            .unwrap();
        let value = store
            .retain_module(&first, ModuleEpochRetention::RuntimeValue)
            .unwrap();
        let artifact = store
            .retain_module(&first, ModuleEpochRetention::CompiledArtifact)
            .unwrap();
        assert!(store.is_program_root(first.key()));
        assert_eq!(store.retention_counts(first.key()).active_calls, 1);
        assert_eq!(store.retention_counts(first.key()).runtime_values, 1);
        assert_eq!(store.retention_counts(first.key()).compiled_artifacts, 1);
        assert!(collect(&store).is_empty());
        assert!(store.loaded(first.key()).is_some());

        drop(call);
        drop(value);
        drop(artifact);
        assert_eq!(collect(&store), vec![first.key()]);
        assert!(store.loaded(first.key()).is_none());
        assert!(store.loaded(second.key()).is_some());
    }
}
