mod applied_layout_identity;
pub(crate) mod collection;
pub(crate) mod constants;
mod descriptor_index;
pub(crate) mod descriptors;
pub mod entry;
pub mod execution;
mod exports;
pub(crate) mod layout_admission;
mod layout_identity;
pub(crate) mod layout_scope;
pub(crate) mod layout_storage;
mod layouts;
pub(crate) mod linked_execution;
mod records;
pub mod retention;
pub(crate) mod staging;
mod state;
#[cfg(test)]
mod tests;
use crate::{
    cache::ReloadDependencySnapshot,
    error::RuntimeError,
    host::{HostPathDescriptorId, HostRegistryId},
    metadata::TypeId,
    module::{
        execution::ExecutionModule,
        layout_admission::{AggregateKind, LayoutAdmissions, LayoutEndpoint},
        layout_identity::{LayoutIdentity, ProgramLayouts},
        layout_scope::LayoutScope,
        records::ModuleRecord,
        retention::{ProgramLease, Retentions},
        staging::{CandidateLease, StagedProgram},
    },
    native::binding::LinkedNativeFunction,
    reload::{ModuleEpoch, ModuleEpochAllocator},
    value::Value,
};
use kagari_bytecode::{
    artifact::{ArtifactFingerprint, ArtifactValidationError, validate_program_resource_limits},
    instruction::{EnumId, PathId, StructId},
    module::BytecodeModule,
    program::{
        BytecodeProgram, ModuleRef, exports::ExportTarget, identity,
        verified::VerifiedBytecodeProgram,
    },
};

use kagari_contract::{
    exports::ProgramExports,
    layout::{EnumLayout, EnumVariantLayout, StructLayout},
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError},
        metadata::DefinitionMetadata,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable, DefinitionView},
    },
};
use std::{
    cell::{BorrowError, RefCell},
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
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
    exports: Arc<ProgramExports>,
    export_targets: Arc<HashMap<DefinitionId, ExportTarget>>,
    root: ModuleRef,
    modules: Arc<[Arc<BytecodeModule>]>,
    definitions: DefinitionTable,
    execution: Arc<[ExecutionModule]>,
    version: Arc<[Arc<BytecodeModule>]>,
    version_definitions: DefinitionTable,
    dependencies: ReloadDependencySnapshot,
}

impl VerifiedProgram {
    /// Import scoped editable records, verify once, and retain immutable evidence.
    pub fn new(
        program: DefinitionMetadata<BytecodeProgram>,
        context: &DefinitionContext,
    ) -> Result<Self, RuntimeError> {
        let invalid = |error: ArtifactValidationError| match error {
            ArtifactValidationError::ResourceLimit(_) => {
                RuntimeError::resource_limit(error.to_string())
            }
            ArtifactValidationError::Bytecode(error) => {
                RuntimeError::module_validation(format!("bytecode validation failed: {error}"))
            }
            error => RuntimeError::module_validation(error.to_string()),
        };
        validate_program_resource_limits(program.records(), program.definitions())
            .map_err(invalid)?;
        let program = if program.definitions().id() == context.snapshot().id() {
            program.into_records()
        } else {
            program
                .import_into(context, &CancellationToken::default())
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?
                .into_records()
        };
        let program = VerifiedBytecodeProgram::new(program, context).map_err(invalid)?;
        Ok(Self::from_bytecode(program))
    }

    /// Adopt bytecode-owned immutable verification evidence. Runtime-local host,
    /// native, ownership and generation checks still run when linking this code.
    pub fn from_bytecode(program: VerifiedBytecodeProgram) -> Self {
        let dependencies =
            ReloadDependencySnapshot::from_program(program.program(), program.definitions())
                .expect("verified bytecode retains its bounded identity scope");
        let definitions = program.definitions().clone();
        let modules: Arc<[Arc<BytecodeModule>]> = program
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
        for (index, prepared) in execution.iter_mut().enumerate() {
            prepared.prepare_calls(ModuleRef::new(index), &modules[index]);
        }
        Self {
            execution: execution.into(),
            root: program.program().root,
            exports: Arc::new(program.program().exports.clone()),
            export_targets: Arc::new(program.export_targets().clone()),
            version: modules.clone(),
            version_definitions: definitions.clone(),
            definitions,
            modules,
            dependencies,
        }
    }

    pub fn root(&self) -> ModuleRef {
        self.root
    }

    pub fn modules(&self) -> &[Arc<BytecodeModule>] {
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
    /// Remapping changes only scoped identity references; no verification flag or hash
    /// can create this evidence, and the original sealed records stay immutable.
    pub(crate) fn normalized(&self, context: &DefinitionContext) -> Result<Self, RuntimeError> {
        if self.definitions.id() == context.snapshot().id() {
            return Ok(self.clone());
        }
        let cancel = CancellationToken::default();
        let mut ids = HashSet::new();
        for module in self.modules.iter() {
            module
                .visit_definition_ids(
                    &mut |id| {
                        ids.insert(*id);
                        Ok(())
                    },
                    &cancel,
                )
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        }
        self.exports
            .visit_definition_ids(
                &mut |id| {
                    ids.insert(*id);
                    Ok(())
                },
                &cancel,
            )
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let (modules, exports, export_targets) = if ids.is_empty() {
            (
                self.modules.clone(),
                self.exports.clone(),
                self.export_targets.clone(),
            )
        } else {
            let remap = context
                .import(&self.definitions, ids)
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let modules = self
                .modules
                .iter()
                .map(|module| {
                    module
                        .remap_definitions(&mut DefinitionMapper::new(
                            &mut |id| remap.map(*id).map_err(DefinitionMappingError::from),
                            &cancel,
                        ))
                        .map(Arc::new)
                })
                .collect::<Result<_, _>>()
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let exports = self
                .exports
                .remap_definitions(&mut DefinitionMapper::new(
                    &mut |id| remap.map(*id).map_err(DefinitionMappingError::from),
                    &cancel,
                ))
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let export_targets = self
                .export_targets
                .iter()
                .map(|(id, target)| remap.map(*id).map(|id| (id, *target)))
                .collect::<Result<_, _>>()
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            (modules, Arc::new(exports), Arc::new(export_targets))
        };
        let definitions = identity::collect(
            &modules.iter().map(Arc::as_ref).collect::<Vec<_>>(),
            context,
        )
        .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        Ok(Self {
            root: self.root,
            exports,
            export_targets,
            modules,
            definitions,
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
    layout_admissions: LayoutAdmissions,
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
    pub bytecode: Arc<BytecodeModule>,
    registry_owner: HostRegistryId,
    host_types: HashMap<DefinitionId, TypeId>,
    pub(crate) host_paths: Vec<HostPathDescriptorId>,
}

#[derive(Debug, Default)]
pub(crate) struct LinkedHostBindings {
    pub types: HashMap<DefinitionId, TypeId>,
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

    /// Copy editable records and their retained identity scope, without the seal.
    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<BytecodeModule>, RuntimeError> {
        DefinitionMetadata::checked(
            self.definitions().clone(),
            self.bytecode.as_ref().clone(),
            cancel,
        )
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
        let root = &self.program.modules[self.program.root.index()];
        ModuleKey {
            id: root.id,
            epoch: root.epoch,
        }
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
    applied: Option<Arc<EnumLayout>>,
    canonical: Option<LayoutIdentity>,
    scope: Option<Arc<LayoutScope>>,
}

impl EnumVariantRef {
    pub(crate) fn registry_owner(&self) -> HostRegistryId {
        self.module.registry_owner
    }

    pub fn layout(&self) -> &EnumLayout {
        self.applied
            .as_deref()
            .unwrap_or(&self.module.bytecode.enumerations[self.id.index()])
    }

    pub fn variant(&self) -> &EnumVariantLayout {
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
    applied: Option<Arc<StructLayout>>,
    canonical: Option<LayoutIdentity>,
    scope: Option<Arc<LayoutScope>>,
}

impl StructLayoutRef {
    /// Clones of one immutable layout can be checked without walking its type
    /// graph, including layouts carrying a generic lexical environment.
    fn same_instance(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.module.program, &other.module.program)
            && self.canonical.is_some()
            && self.canonical == other.canonical
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
            && match (&self.scope, &other.scope) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }

    pub fn layout(&self) -> &StructLayout {
        self.applied
            .as_deref()
            .unwrap_or(&self.module.bytecode.structures[self.id.index()])
    }

    pub fn module(&self) -> &LoadedModule {
        &self.module
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.same_instance(other)
            || layout_admission::admit(
                AggregateKind::Struct,
                LayoutEndpoint {
                    owner: &self.module,
                    identity: self.canonical,
                },
                LayoutEndpoint {
                    owner: &other.module,
                    identity: other.canonical,
                },
                || {
                    self.matches_type(
                        &other.type_expression(),
                        &other.module,
                        other.type_bindings().map(Arc::as_ref),
                    )
                },
            )
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
    abandoned: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
struct ModuleStoreInner {
    epochs: ModuleEpochAllocator,
    next_id: usize,
    ids_by_member: HashMap<(String, ModuleIdentity), ModuleId>,
    records: HashMap<ModuleKey, ModuleRecord>,
    latest_by_name: HashMap<String, ModuleKey>,
    staged: HashMap<ModuleKey, Weak<CandidateLease>>,
    retentions: HashMap<ModuleKey, Retentions>,
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
                    host_paths: paths,
                }
            })
            .collect();
        let program = Arc::new(ProgramDescriptor {
            layouts: ProgramLayouts::prepare(&program.modules),
            layout_admissions: LayoutAdmissions::default(),
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
        let lease = Arc::new(CandidateLease::new(self.abandoned.clone()));
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

    pub(crate) fn abandonment_pending(&self) -> bool {
        self.abandoned.load(Ordering::Acquire)
    }

    /// External safepoints admit storage even when no reclamation is pending.
    /// Closed regions admit no external store borrows between polls.
    pub(crate) fn check_collection_access(&self) -> Result<(), RuntimeError> {
        let _storage = self.inner.try_borrow().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed at a collection safepoint")
        })?;
        Ok(())
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
