use kagari_bytecode::{
    artifact::{
        ArtifactFingerprint, DependencyFingerprint, KbcArtifact, PathDescriptorFingerprint,
        PublicAbiFingerprint,
    },
    module::BytecodeModule,
    program::BytecodeProgram,
};
use kagari_common::identity::{mapping::DefinitionMappingError, table::DefinitionTable};
use std::{
    cell::{RefCell, RefMut},
    collections::HashMap,
};

use {kagari_abi::version::KAGARI_RUNTIME_HELPER_ABI_VERSION, kagari_contract::ids::FunctionRef};

use crate::{
    error::RuntimeError,
    module::{InstallationId, ModuleKey},
    reload::{ModuleEpoch, path_fingerprints_for_module, public_abi_fingerprints_for_module},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterpreterCacheId(u64);

impl InterpreterCacheId {
    pub fn index(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadDependencySnapshot {
    pub module_fingerprint: ArtifactFingerprint,
    pub public_abi_fingerprints: Vec<PublicAbiFingerprint>,
    pub typed_path_fingerprints: Vec<PathDescriptorFingerprint>,
    pub dependency_fingerprints: Vec<DependencyFingerprint>,
    pub runtime_helper_abi_version: String,
}

impl ReloadDependencySnapshot {
    pub fn from_program(
        program: &BytecodeProgram,
        definitions: &DefinitionTable,
    ) -> Result<Self, DefinitionMappingError> {
        let mut snapshot =
            Self::from_bytecode(&program.modules[program.root.index()], definitions)?;
        snapshot.module_fingerprint = ArtifactFingerprint::of_program(program, definitions)?;
        snapshot.dependency_fingerprints = program.dependency_fingerprints(definitions)?;
        Ok(snapshot)
    }

    pub fn from_bytecode(
        module: &BytecodeModule,
        definitions: &DefinitionTable,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            module_fingerprint: ArtifactFingerprint::of_module(module, definitions)?,
            public_abi_fingerprints: public_abi_fingerprints_for_module(module, definitions)?,
            typed_path_fingerprints: path_fingerprints_for_module(module),
            dependency_fingerprints: Vec::new(),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.to_owned(),
        })
    }

    pub fn from_artifact(
        artifact: &KbcArtifact,
        definitions: &DefinitionTable,
    ) -> Result<Self, DefinitionMappingError> {
        Self::from_program(&artifact.program, definitions)
    }
}

/// Runtime-local interpreter cache metadata. Records neither own native code nor
/// retain module epochs; callers must check reachability before using cached data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpreterCacheRecord {
    pub id: InterpreterCacheId,
    pub installation: InstallationId,
    pub module: ModuleKey,
    pub function: Option<FunctionRef>,
    pub dependencies: ReloadDependencySnapshot,
}

/// Invalidation bookkeeping for interpreter caches. Native products are owned by
/// prepared programs and runtime installations retain their exact pinned versions.
#[derive(Debug, Default)]
pub struct InterpreterCacheRegistry {
    inner: RefCell<InterpreterCacheRegistryInner>,
}

#[derive(Debug, Default)]
struct InterpreterCacheRegistryInner {
    next_id: u64,
    entries: HashMap<InterpreterCacheId, InterpreterCacheRecord>,
}

impl InterpreterCacheRegistry {
    pub fn register(
        &self,
        installation: InstallationId,
        module: ModuleKey,
        function: Option<FunctionRef>,
        dependencies: ReloadDependencySnapshot,
    ) -> InterpreterCacheId {
        let mut inner = self.inner.borrow_mut();
        let id = InterpreterCacheId(inner.next_id);
        inner.next_id += 1;
        inner.entries.insert(
            id,
            InterpreterCacheRecord {
                id,
                installation,
                module,
                function,
                dependencies,
            },
        );
        id
    }

    pub fn get(&self, id: InterpreterCacheId) -> Option<InterpreterCacheRecord> {
        self.inner.borrow().entries.get(&id).cloned()
    }

    pub(crate) fn prepare_invalidation(
        &self,
        installation: InstallationId,
        published: ModuleEpoch,
    ) -> Result<CacheInvalidation<'_>, RuntimeError> {
        let inner = self.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("interpreter caches borrowed during publication")
        })?;
        let mut invalidated = Vec::new();
        invalidated
            .try_reserve(inner.entries.len())
            .map_err(|_| RuntimeError::resource_limit("interpreter cache invalidation"))?;
        invalidated.extend(inner.entries.iter().filter_map(|(id, cache)| {
            (cache.installation == installation && cache.module.epoch != published).then_some(*id)
        }));
        Ok(CacheInvalidation { inner, invalidated })
    }
}

/// Preallocated invalidation under an exclusive registry borrow. Dropping an
/// uncommitted plan leaves caches unchanged if publication rejects the candidate.
pub(crate) struct CacheInvalidation<'a> {
    inner: RefMut<'a, InterpreterCacheRegistryInner>,
    invalidated: Vec<InterpreterCacheId>,
}

impl CacheInvalidation<'_> {
    pub(crate) fn commit(mut self) {
        for id in self.invalidated {
            self.inner.entries.remove(&id);
        }
    }
}
