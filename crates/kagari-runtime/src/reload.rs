use kagari_bytecode::{
    artifact::{
        ArtifactCompatibility, ArtifactFingerprint, ArtifactValidationError, KbcArtifact,
        PathDescriptorFingerprint, PublicAbiFingerprint, validate_program_resource_limits,
    },
    module::BytecodeModule,
    program::{BytecodeProgram, verify_program},
    verifier::BytecodeVerificationError,
};

use std::collections::{BTreeMap, HashMap};

use crate::{
    error::RuntimeError,
    module::{LoadedModule, ModuleId, VerifiedProgram},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleEpoch(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReloadValidationError {
    #[error("reload module identity mismatch: expected `{expected}`, found `{found}`")]
    ModuleIdentityMismatch { expected: String, found: String },
    #[error("reload module id changed: expected {expected:?}, found {found:?}")]
    ModuleIdChanged { expected: ModuleId, found: ModuleId },
    #[error(
        "reload target `{module_name}` is not active: expected {expected:?}, active {active:?}"
    )]
    ModuleNotActive {
        module_name: String,
        expected: ModuleEpoch,
        active: Option<ModuleEpoch>,
    },
    #[error("reload artifact validation failed: {0}")]
    Artifact(ArtifactValidationError),
    #[error("reload bytecode validation failed: {0}")]
    Bytecode(BytecodeVerificationError),
    #[error("reload runtime validation failed: {0}")]
    Runtime(RuntimeError),
    #[error("reload public ABI fingerprints changed")]
    PublicAbiFingerprintMismatch,
    #[error("reload typed path fingerprints changed")]
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

pub fn validate_load_candidate(bytecode: &BytecodeProgram) -> Result<(), ReloadValidationError> {
    validate_program_resource_limits(bytecode).map_err(ReloadValidationError::Artifact)?;
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
    validate_reload_target(active, candidate_name, active_latest)?;
    validate_load_candidate(candidate)?;
    validate_reload_contracts(
        active,
        &candidate.modules[candidate.root.index()],
        candidate.modules.iter(),
    )
}

pub(crate) fn validate_verified_reload_candidate(
    active: &LoadedModule,
    candidate_name: &str,
    candidate: &VerifiedProgram,
    active_latest: Option<&LoadedModule>,
) -> Result<(), ReloadValidationError> {
    validate_reload_target(active, candidate_name, active_latest)?;
    let modules = candidate
        .modules()
        .iter()
        .map(|module| candidate.paths(module.as_ref()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(ReloadValidationError::Runtime)?;
    validate_reload_contracts(active, &modules[candidate.root().index()], modules.iter())
}

fn validate_reload_target(
    active: &LoadedModule,
    candidate_name: &str,
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

    Ok(())
}

fn validate_reload_contracts<'a>(
    active: &LoadedModule,
    root: &BytecodeModule,
    modules: impl ExactSizeIterator<Item = &'a BytecodeModule>,
) -> Result<(), ReloadValidationError> {
    if active.bytecode.identity != root.identity {
        return Err(ReloadValidationError::ModuleIdentityMismatch {
            expected: active.bytecode.identity.to_string(),
            found: root.identity.to_string(),
        });
    }
    let current = active
        .members()
        .map(|module| (module.bytecode.identity.clone(), module))
        .collect::<BTreeMap<_, _>>();
    if current.len() != modules.len() {
        return Err(ReloadValidationError::PublicAbiFingerprintMismatch);
    }
    for module in modules {
        let previous = current
            .get(&module.identity)
            .ok_or(ReloadValidationError::PublicAbiFingerprintMismatch)?;
        let previous_bytecode = previous
            .verified_program()
            .paths(previous.bytecode.as_ref())
            .map_err(ReloadValidationError::Runtime)?;
        if public_abi_fingerprints_for_module(&previous_bytecode)
            != public_abi_fingerprints_for_module(module)
        {
            return Err(ReloadValidationError::PublicAbiFingerprintMismatch);
        }
        if path_fingerprints_for_module(&previous_bytecode) != path_fingerprints_for_module(module)
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

/// Reserves version identities independently of activation. Failed candidates may
/// leave gaps; an identity is never reused for a later candidate.
#[derive(Debug, Default)]
pub(crate) struct ModuleEpochAllocator {
    epochs: HashMap<String, ModuleEpoch>,
}

impl ModuleEpochAllocator {
    pub(crate) fn reserve(&mut self, module_name: &str) -> Result<ModuleEpoch, RuntimeError> {
        let previous = self.epochs.get(module_name).map_or(0, |epoch| epoch.0);
        let next = previous
            .checked_add(1)
            .ok_or_else(|| RuntimeError::module_validation("module epoch space exhausted"))?;
        let epoch = ModuleEpoch(next);
        self.epochs.insert(module_name.to_string(), epoch);
        Ok(epoch)
    }
}

#[cfg(test)]
mod epoch_tests {
    use kagari_bytecode::program::ModuleRef;

    use super::*;

    #[test]
    fn reload_preflight_rejects_oversized_in_memory_programs() {
        let oversized = BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default(); 1_025],
        };
        assert!(matches!(
            validate_load_candidate(&oversized),
            Err(ReloadValidationError::Artifact(
                ArtifactValidationError::ResourceLimit(_)
            ))
        ));
    }

    #[test]
    fn exhausted_epoch_space_is_rejected_without_reusing_an_identity() {
        let mut allocator = ModuleEpochAllocator::default();
        assert_eq!(allocator.reserve("main").unwrap(), ModuleEpoch(1));
        assert_eq!(allocator.reserve("main").unwrap(), ModuleEpoch(2));
        allocator
            .epochs
            .insert("main".into(), ModuleEpoch(u64::MAX));
        assert!(allocator.reserve("main").is_err());
        assert_eq!(allocator.epochs["main"], ModuleEpoch(u64::MAX));
        assert_eq!(allocator.reserve("other").unwrap(), ModuleEpoch(1));
    }
}
