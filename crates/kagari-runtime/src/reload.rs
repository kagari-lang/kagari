use std::collections::HashMap;

use kagari_ir::bytecode::{
    ArtifactCompatibility, ArtifactFingerprint, ArtifactValidationError, BytecodeModule,
    BytecodeProgram, BytecodeVerificationError, KbcArtifact, PathDescriptorFingerprint,
    PublicAbiFingerprint, verify_program,
};

use crate::{
    error::RuntimeError,
    module::{LoadedModule, ModuleId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleEpoch(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadValidationError {
    ModuleIdentityMismatch {
        expected: String,
        found: String,
    },
    ModuleIdChanged {
        expected: ModuleId,
        found: ModuleId,
    },
    ModuleNotActive {
        module_name: String,
        expected: ModuleEpoch,
        active: Option<ModuleEpoch>,
    },
    Artifact(ArtifactValidationError),
    Bytecode(BytecodeVerificationError),
    Runtime(RuntimeError),
    PublicAbiFingerprintMismatch,
    PathFingerprintMismatch,
}

impl ReloadValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ModuleIdentityMismatch { .. } => "KG_RELOAD_MODULE_IDENTITY_MISMATCH",
            Self::ModuleIdChanged { .. } => "KG_RELOAD_MODULE_ID_CHANGED",
            Self::ModuleNotActive { .. } => "KG_RELOAD_MODULE_NOT_ACTIVE",
            Self::Artifact(error) => error.code(),
            Self::Bytecode(error) => error.code(),
            Self::Runtime(error) => error.code(),
            Self::PublicAbiFingerprintMismatch => "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH",
            Self::PathFingerprintMismatch => "KG_RELOAD_PATH_FINGERPRINT_MISMATCH",
        }
    }
}

impl std::fmt::Display for ReloadValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModuleIdentityMismatch { expected, found } => write!(
                f,
                "reload module identity mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::ModuleIdChanged { expected, found } => write!(
                f,
                "reload module id changed: expected {:?}, found {:?}",
                expected, found
            ),
            Self::ModuleNotActive {
                module_name,
                expected,
                active,
            } => write!(
                f,
                "reload target `{module_name}` is not active: expected {:?}, active {:?}",
                expected, active
            ),
            Self::Artifact(error) => write!(f, "reload artifact validation failed: {error}"),
            Self::Bytecode(error) => write!(f, "reload bytecode validation failed: {error}"),
            Self::Runtime(error) => write!(f, "reload runtime validation failed: {error}"),
            Self::PublicAbiFingerprintMismatch => {
                write!(f, "reload public ABI fingerprints changed")
            }
            Self::PathFingerprintMismatch => write!(f, "reload typed path fingerprints changed"),
        }
    }
}

impl std::error::Error for ReloadValidationError {}

pub fn validate_load_candidate(bytecode: &BytecodeProgram) -> Result<(), ReloadValidationError> {
    verify_program(bytecode).map_err(ReloadValidationError::Bytecode)
}

pub fn validate_reload_artifact_candidate(
    active: &LoadedModule,
    candidate_name: &str,
    artifact: &KbcArtifact,
    compatibility: &ArtifactCompatibility,
    active_latest: Option<&LoadedModule>,
) -> Result<(), ReloadValidationError> {
    artifact
        .validate_for_loader(compatibility)
        .map_err(ReloadValidationError::Artifact)?;
    validate_reload_candidate(active, candidate_name, &artifact.program, active_latest)
}

pub fn validate_reload_candidate(
    active: &LoadedModule,
    candidate_name: &str,
    candidate: &BytecodeProgram,
    active_latest: Option<&LoadedModule>,
) -> Result<(), ReloadValidationError> {
    if candidate_name != active.name {
        return Err(ReloadValidationError::ModuleIdentityMismatch {
            expected: active.name.clone(),
            found: candidate_name.to_owned(),
        });
    }
    let Some(latest) = active_latest else {
        return Err(ReloadValidationError::ModuleNotActive {
            module_name: active.name.clone(),
            expected: active.epoch,
            active: None,
        });
    };
    if latest.id != active.id {
        return Err(ReloadValidationError::ModuleIdChanged {
            expected: active.id,
            found: latest.id,
        });
    }
    if latest.epoch != active.epoch {
        return Err(ReloadValidationError::ModuleNotActive {
            module_name: active.name.clone(),
            expected: active.epoch,
            active: Some(latest.epoch),
        });
    }

    validate_load_candidate(candidate)?;
    let root = &candidate.modules[candidate.root.index()];
    if active.bytecode.identity != root.identity {
        return Err(ReloadValidationError::ModuleIdentityMismatch {
            expected: active.bytecode.identity.to_string(),
            found: root.identity.to_string(),
        });
    }
    let current = active
        .members()
        .map(|module| (module.bytecode.identity.clone(), module))
        .collect::<std::collections::BTreeMap<_, _>>();
    if current.len() != candidate.modules.len() {
        return Err(ReloadValidationError::PublicAbiFingerprintMismatch);
    }
    for module in &candidate.modules {
        let previous = current
            .get(&module.identity)
            .ok_or(ReloadValidationError::PublicAbiFingerprintMismatch)?;
        if public_abi_fingerprints_for_module(&previous.bytecode)
            != public_abi_fingerprints_for_module(module)
        {
            return Err(ReloadValidationError::PublicAbiFingerprintMismatch);
        }
        if path_fingerprints_for_module(&previous.bytecode) != path_fingerprints_for_module(module)
        {
            return Err(ReloadValidationError::PathFingerprintMismatch);
        }
    }
    Ok(())
}

pub fn public_abi_fingerprints_for_module(module: &BytecodeModule) -> Vec<PublicAbiFingerprint> {
    module
        .public_items
        .iter()
        .map(|item| PublicAbiFingerprint {
            name: item.fingerprint_name(),
            fingerprint: ArtifactFingerprint::of_serialized(item),
        })
        .collect()
}

pub fn path_fingerprints_for_module(module: &BytecodeModule) -> Vec<PathDescriptorFingerprint> {
    module
        .paths
        .iter()
        .map(|path| PathDescriptorFingerprint {
            path: path.id,
            fingerprint: ArtifactFingerprint::of_serialized(path),
        })
        .collect()
}

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
