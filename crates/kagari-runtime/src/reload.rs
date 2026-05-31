use std::collections::HashMap;

use kagari_ir::bytecode::{
    ArtifactFingerprint, BytecodeModule, BytecodeVerificationError, PathDescriptorFingerprint,
    PublicAbiFingerprint, verify_module,
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
    Bytecode(BytecodeVerificationError),
    Runtime(RuntimeError),
    PublicAbiFingerprintMismatch,
    PathFingerprintMismatch,
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
            Self::Bytecode(error) => write!(f, "reload bytecode validation failed: {error:?}"),
            Self::Runtime(error) => write!(f, "reload runtime validation failed: {error}"),
            Self::PublicAbiFingerprintMismatch => {
                write!(f, "reload public ABI fingerprints changed")
            }
            Self::PathFingerprintMismatch => write!(f, "reload typed path fingerprints changed"),
        }
    }
}

impl std::error::Error for ReloadValidationError {}

pub fn validate_load_candidate(bytecode: &BytecodeModule) -> Result<(), ReloadValidationError> {
    verify_module(bytecode).map_err(ReloadValidationError::Bytecode)
}

pub fn validate_reload_candidate(
    active: &LoadedModule,
    candidate_name: &str,
    candidate: &BytecodeModule,
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
    if public_abi_fingerprints_for_module(&active.bytecode)
        != public_abi_fingerprints_for_module(candidate)
    {
        return Err(ReloadValidationError::PublicAbiFingerprintMismatch);
    }
    if path_fingerprints_for_module(&active.bytecode) != path_fingerprints_for_module(candidate) {
        return Err(ReloadValidationError::PathFingerprintMismatch);
    }
    Ok(())
}

pub fn public_abi_fingerprints_for_module(module: &BytecodeModule) -> Vec<PublicAbiFingerprint> {
    module
        .public_items
        .iter()
        .map(|item| PublicAbiFingerprint {
            name: item.fingerprint_name(),
            fingerprint: ArtifactFingerprint::of_debug(item),
        })
        .collect()
}

pub fn path_fingerprints_for_module(module: &BytecodeModule) -> Vec<PathDescriptorFingerprint> {
    module
        .paths
        .iter()
        .map(|path| PathDescriptorFingerprint {
            path: path.id,
            fingerprint: ArtifactFingerprint::of_debug(path),
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
