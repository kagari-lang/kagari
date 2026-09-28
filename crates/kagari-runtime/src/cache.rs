use kagari_bytecode::{
    ArtifactFingerprint, BytecodeModule, BytecodeProgram, DependencyFingerprint, KbcArtifact,
    PathDescriptorFingerprint, PublicAbiFingerprint,
};
use kagari_common::identity::ModuleIdentity;
use std::{cell::RefCell, collections::HashMap};

use kagari_abi::{ids::FunctionRef, version::KAGARI_RUNTIME_HELPER_ABI_VERSION};

use crate::{
    module::{ModuleId, ModuleKey},
    reload::{path_fingerprints_for_module, public_abi_fingerprints_for_module},
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
    pub host_interface_fingerprint: ArtifactFingerprint,
    pub runtime_helper_abi_version: String,
}

impl ReloadDependencySnapshot {
    pub fn from_program(program: &BytecodeProgram) -> Self {
        let mut snapshot = Self::from_bytecode(&program.modules[program.root.index()]);
        snapshot.module_fingerprint = ArtifactFingerprint::of_serialized(program);
        snapshot.host_interface_fingerprint = ArtifactFingerprint::of_program_hosts(program);
        snapshot.dependency_fingerprints = program.dependency_fingerprints();
        snapshot
    }
    pub fn from_bytecode(module: &BytecodeModule) -> Self {
        Self {
            module_fingerprint: ArtifactFingerprint::of_serialized(module),
            public_abi_fingerprints: public_abi_fingerprints_for_module(module),
            typed_path_fingerprints: path_fingerprints_for_module(module),
            dependency_fingerprints: Vec::new(),
            host_interface_fingerprint: ArtifactFingerprint::of_host_interface(
                &module.host_interface,
            ),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.to_owned(),
        }
    }

    pub fn from_artifact(artifact: &KbcArtifact) -> Self {
        Self::from_program(&artifact.program)
    }
}

/// Runtime-local interpreter cache metadata. Records neither own native code nor
/// retain module epochs; callers must check reachability before using cached data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpreterCacheRecord {
    pub id: InterpreterCacheId,
    pub module: ModuleKey,
    pub function: Option<FunctionRef>,
    pub dependencies: ReloadDependencySnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadInvalidation {
    pub module_name: String,
    pub module_identity: ModuleIdentity,
    pub module_fingerprint: ArtifactFingerprint,
    pub module_id: ModuleId,
    pub published: ModuleKey,
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

    pub fn invalidate_for_reload(
        &self,
        invalidation: &ReloadInvalidation,
    ) -> Vec<InterpreterCacheRecord> {
        let mut inner = self.inner.borrow_mut();
        let mut invalidated = inner
            .entries
            .iter()
            .filter_map(|(id, cache)| {
                cache_invalidated_by_reload(cache, invalidation).then_some(*id)
            })
            .collect::<Vec<_>>();
        invalidated.sort_by_key(|id| id.index());
        invalidated
            .into_iter()
            .filter_map(|id| inner.entries.remove(&id))
            .collect()
    }
}

fn cache_invalidated_by_reload(
    cache: &InterpreterCacheRecord,
    invalidation: &ReloadInvalidation,
) -> bool {
    if cache.module.id == invalidation.module_id {
        if cache.module.epoch == invalidation.published.epoch {
            return false;
        }
        return true;
    }

    cache
        .dependencies
        .dependency_fingerprints
        .iter()
        .any(|dependency| {
            dependency.module_id == invalidation.module_identity
                && dependency.fingerprint != invalidation.module_fingerprint
        })
}
