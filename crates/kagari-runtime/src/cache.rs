use std::{cell::RefCell, collections::HashMap};

use kagari_ir::bytecode::{
    ArtifactFingerprint, BytecodeModule, DependencyFingerprint, FunctionRef,
    KAGARI_RUNTIME_HELPER_ABI_VERSION, KbcArtifact, PathDescriptorFingerprint,
    PublicAbiFingerprint,
};

use crate::{
    backend::ExecutableFunctionArtifact,
    module::{ModuleId, ModuleKey},
    reload::{path_fingerprints_for_module, public_abi_fingerprints_for_module},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutionArtifactId(u64);

impl ExecutionArtifactId {
    pub fn index(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionArtifactKind {
    InterpreterCache,
    Jit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadDependencySnapshot {
    pub module_fingerprint: ArtifactFingerprint,
    pub public_abi_fingerprints: Vec<PublicAbiFingerprint>,
    pub typed_path_fingerprints: Vec<PathDescriptorFingerprint>,
    pub dependency_fingerprints: Vec<DependencyFingerprint>,
    pub host_registry_fingerprint: ArtifactFingerprint,
    pub runtime_helper_abi_version: String,
}

impl ReloadDependencySnapshot {
    pub fn from_bytecode(module: &BytecodeModule) -> Self {
        Self {
            module_fingerprint: ArtifactFingerprint::of_debug(module),
            public_abi_fingerprints: public_abi_fingerprints_for_module(module),
            typed_path_fingerprints: path_fingerprints_for_module(module),
            dependency_fingerprints: Vec::new(),
            host_registry_fingerprint: ArtifactFingerprint::empty(),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.to_owned(),
        }
    }

    pub fn from_artifact(artifact: &KbcArtifact) -> Self {
        Self {
            module_fingerprint: artifact.header.content_hash,
            public_abi_fingerprints: artifact.verification.public_abi_fingerprints.clone(),
            typed_path_fingerprints: artifact.verification.typed_path_fingerprints.clone(),
            dependency_fingerprints: artifact.verification.dependency_fingerprints.clone(),
            host_registry_fingerprint: artifact.verification.host_registry_fingerprint,
            runtime_helper_abi_version: artifact.header.runtime_helper_abi_version.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionArtifactRecord {
    pub id: ExecutionArtifactId,
    pub kind: ExecutionArtifactKind,
    pub module: ModuleKey,
    pub function: Option<FunctionRef>,
    pub dependencies: ReloadDependencySnapshot,
    pub executable: Option<ExecutableFunctionArtifact>,
    pub valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadInvalidation {
    pub module_name: String,
    pub module_id: ModuleId,
    pub published: ModuleKey,
    pub dependencies: ReloadDependencySnapshot,
}

#[derive(Debug, Default)]
pub struct ExecutionArtifactRegistry {
    inner: RefCell<ExecutionArtifactRegistryInner>,
}

#[derive(Debug, Default)]
struct ExecutionArtifactRegistryInner {
    next_id: u64,
    artifacts: HashMap<ExecutionArtifactId, ExecutionArtifactRecord>,
}

impl ExecutionArtifactRegistry {
    pub fn register(
        &self,
        kind: ExecutionArtifactKind,
        module: ModuleKey,
        function: Option<FunctionRef>,
        dependencies: ReloadDependencySnapshot,
    ) -> ExecutionArtifactId {
        let mut inner = self.inner.borrow_mut();
        let id = ExecutionArtifactId(inner.next_id);
        inner.next_id += 1;
        inner.artifacts.insert(
            id,
            ExecutionArtifactRecord {
                id,
                kind,
                module,
                function,
                dependencies,
                executable: None,
                valid: true,
            },
        );
        id
    }

    pub fn register_executable_function(
        &self,
        module: ModuleKey,
        dependencies: ReloadDependencySnapshot,
        executable: ExecutableFunctionArtifact,
    ) -> ExecutionArtifactId {
        let mut inner = self.inner.borrow_mut();
        let id = ExecutionArtifactId(inner.next_id);
        inner.next_id += 1;
        let function = Some(executable.function);
        inner.artifacts.insert(
            id,
            ExecutionArtifactRecord {
                id,
                kind: ExecutionArtifactKind::Jit,
                module,
                function,
                dependencies,
                executable: Some(executable),
                valid: true,
            },
        );
        id
    }

    pub fn get(&self, id: ExecutionArtifactId) -> Option<ExecutionArtifactRecord> {
        self.inner.borrow().artifacts.get(&id).cloned()
    }

    pub fn invalidate_for_reload(
        &self,
        invalidation: &ReloadInvalidation,
    ) -> Vec<ExecutionArtifactRecord> {
        let mut inner = self.inner.borrow_mut();
        let mut invalidated = Vec::new();
        for artifact in inner.artifacts.values_mut() {
            if !artifact.valid || !artifact_invalidated_by_reload(artifact, invalidation) {
                continue;
            }
            artifact.valid = false;
            invalidated.push(artifact.clone());
        }
        invalidated
    }
}

fn artifact_invalidated_by_reload(
    artifact: &ExecutionArtifactRecord,
    invalidation: &ReloadInvalidation,
) -> bool {
    if artifact.module.id == invalidation.module_id {
        if artifact.module.epoch == invalidation.published.epoch {
            return false;
        }
        return artifact.dependencies.public_abi_fingerprints
            != invalidation.dependencies.public_abi_fingerprints
            || artifact.dependencies.typed_path_fingerprints
                != invalidation.dependencies.typed_path_fingerprints
            || artifact.dependencies.dependency_fingerprints
                != invalidation.dependencies.dependency_fingerprints
            || artifact.dependencies.host_registry_fingerprint
                != invalidation.dependencies.host_registry_fingerprint
            || artifact.dependencies.runtime_helper_abi_version
                != invalidation.dependencies.runtime_helper_abi_version;
    }

    artifact
        .dependencies
        .dependency_fingerprints
        .iter()
        .any(|dependency| {
            dependency.module_id == invalidation.module_name
                && dependency.fingerprint != invalidation.dependencies.module_fingerprint
        })
}
