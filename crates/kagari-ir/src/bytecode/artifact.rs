use crate::{
    bytecode::{BytecodeDebugMetadata, BytecodeModule, FunctionRef, PathId, verify_module},
    module::ValueType,
};

pub const KBC_MAGIC: [u8; 4] = *b"KBC\0";
pub const KBC_ARTIFACT_FORMAT_VERSION: u16 = 1;
pub const KAGARI_LANGUAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const KAGARI_COMPILER_FINGERPRINT: &str = concat!("kagari-ir/", env!("CARGO_PKG_VERSION"));
pub const KAGARI_RUNTIME_ABI_VERSION: &str = "kagari-runtime-abi-v1";
pub const KAGARI_RUNTIME_HELPER_ABI_VERSION: &str = "kagari-runtime-helper-abi-v1";

#[derive(Debug, Clone)]
pub struct KbcArtifact {
    pub header: ArtifactHeader,
    pub module: BytecodeModule,
    pub tables: ArtifactTables,
    pub verification: VerificationMetadata,
    pub debug: Option<DebugMetadata>,
    pub signatures: Option<ArtifactSignatures>,
}

impl KbcArtifact {
    pub fn from_module(module: BytecodeModule, options: ArtifactBuildOptions) -> Self {
        let mut tables = ArtifactTables::from_module(&module);
        let verification = VerificationMetadata::from_module(&module, &options);
        let debug = options.debug;
        let signatures = options.signatures;
        if let Some(debug) = &debug {
            push_section(
                &mut tables.sections,
                ArtifactSectionId::Debug,
                debug.source_files.len() + debug.debug_names.len(),
            );
            tables.source_files = debug.source_files.clone();
            tables.debug_names = debug.debug_names.clone();
        }
        if let Some(signatures) = &signatures {
            push_section(
                &mut tables.sections,
                ArtifactSectionId::Signatures,
                signatures.signatures.len(),
            );
        }
        let mut artifact = Self {
            header: ArtifactHeader {
                magic: KBC_MAGIC,
                format_version: KBC_ARTIFACT_FORMAT_VERSION,
                language_version: KAGARI_LANGUAGE_VERSION.to_owned(),
                compiler_fingerprint: KAGARI_COMPILER_FINGERPRINT.to_owned(),
                runtime_abi_version: options.runtime_abi_version,
                runtime_helper_abi_version: options.runtime_helper_abi_version,
                encoding: ArtifactEncoding::CanonicalLittleEndian,
                module_identity: options.module_identity,
                module_epoch: options.module_epoch,
                content_hash: ArtifactFingerprint::empty(),
            },
            module,
            tables,
            verification,
            debug,
            signatures,
        };
        artifact.header.content_hash = artifact.compute_content_hash();
        artifact
    }

    pub fn validate_for_loader(
        &self,
        requirements: &ArtifactCompatibility,
    ) -> Result<(), ArtifactValidationError> {
        self.validate_header(requirements)?;
        if self.header.content_hash != self.compute_content_hash() {
            return Err(ArtifactValidationError::ContentHashMismatch);
        }
        if let Some(expected_module) = &requirements.module_identity
            && &self.header.module_identity != expected_module
        {
            return Err(ArtifactValidationError::ModuleIdentityMismatch {
                expected: expected_module.clone(),
                found: self.header.module_identity.clone(),
            });
        }
        if self.verification.loader.module_identity != self.header.module_identity {
            return Err(ArtifactValidationError::ModuleIdentityMismatch {
                expected: self.header.module_identity.clone(),
                found: self.verification.loader.module_identity.clone(),
            });
        }
        if self.verification.loader.runtime_abi_version != self.header.runtime_abi_version {
            return Err(ArtifactValidationError::RuntimeAbiMismatch {
                expected: self.header.runtime_abi_version.clone(),
                found: self.verification.loader.runtime_abi_version.clone(),
            });
        }
        if self.verification.loader.runtime_helper_abi_version
            != self.header.runtime_helper_abi_version
        {
            return Err(ArtifactValidationError::RuntimeHelperAbiMismatch {
                expected: self.header.runtime_helper_abi_version.clone(),
                found: self.verification.loader.runtime_helper_abi_version.clone(),
            });
        }
        verify_module(&self.module).map_err(ArtifactValidationError::Bytecode)?;
        if !self.verification.bytecode_verified {
            return Err(ArtifactValidationError::UnverifiedBytecode);
        }
        if self.verification.loader.dependency_fingerprints != requirements.dependency_fingerprints
        {
            return Err(ArtifactValidationError::DependencyFingerprintMismatch);
        }
        if self.verification.loader.host_registry_fingerprint
            != requirements.host_registry_fingerprint
        {
            return Err(ArtifactValidationError::HostRegistryFingerprintMismatch {
                expected: requirements.host_registry_fingerprint,
                found: self.verification.loader.host_registry_fingerprint,
            });
        }
        if self.verification.loader.security_profile != requirements.security_profile {
            return Err(ArtifactValidationError::SecurityProfileMismatch {
                expected: requirements.security_profile.clone(),
                found: self.verification.loader.security_profile.clone(),
            });
        }
        if self.verification.typed_path_fingerprints
            != self.verification.loader.typed_path_fingerprints
        {
            return Err(ArtifactValidationError::PathFingerprintMismatch);
        }
        if self.verification.public_abi_fingerprints
            != self.verification.loader.public_abi_fingerprints
        {
            return Err(ArtifactValidationError::PublicAbiFingerprintMismatch);
        }
        Ok(())
    }

    fn validate_header(
        &self,
        requirements: &ArtifactCompatibility,
    ) -> Result<(), ArtifactValidationError> {
        if self.header.magic != KBC_MAGIC {
            return Err(ArtifactValidationError::InvalidMagic(self.header.magic));
        }
        if self.header.format_version != requirements.format_version {
            return Err(ArtifactValidationError::FormatVersionMismatch {
                expected: requirements.format_version,
                found: self.header.format_version,
            });
        }
        if self.header.language_version != requirements.language_version {
            return Err(ArtifactValidationError::LanguageVersionMismatch {
                expected: requirements.language_version.clone(),
                found: self.header.language_version.clone(),
            });
        }
        if self.header.runtime_abi_version != requirements.runtime_abi_version {
            return Err(ArtifactValidationError::RuntimeAbiMismatch {
                expected: requirements.runtime_abi_version.clone(),
                found: self.header.runtime_abi_version.clone(),
            });
        }
        if self.header.runtime_helper_abi_version != requirements.runtime_helper_abi_version {
            return Err(ArtifactValidationError::RuntimeHelperAbiMismatch {
                expected: requirements.runtime_helper_abi_version.clone(),
                found: self.header.runtime_helper_abi_version.clone(),
            });
        }
        Ok(())
    }

    fn compute_content_hash(&self) -> ArtifactFingerprint {
        ArtifactFingerprint::of_debug(&(
            &self.module,
            &self.tables,
            &self.verification,
            &self.debug,
            &self.signatures,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactHeader {
    pub magic: [u8; 4],
    pub format_version: u16,
    pub language_version: String,
    pub compiler_fingerprint: String,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub encoding: ArtifactEncoding,
    pub module_identity: ArtifactModuleIdentity,
    pub module_epoch: Option<ModuleEpoch>,
    pub content_hash: ArtifactFingerprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactEncoding {
    CanonicalLittleEndian,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactModuleIdentity {
    pub package_id: String,
    pub module_path: String,
    pub source_uri: String,
    pub module_id: String,
}

impl ArtifactModuleIdentity {
    pub fn single_file(source_uri: impl Into<String>) -> Self {
        let source_uri = source_uri.into();
        Self {
            package_id: "root".to_owned(),
            module_path: "main".to_owned(),
            module_id: ArtifactFingerprint::of_str(&source_uri).to_hex(),
            source_uri,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleEpoch(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArtifactFingerprint(pub u64);

impl ArtifactFingerprint {
    pub fn empty() -> Self {
        Self(0)
    }

    pub fn of_debug(value: &impl std::fmt::Debug) -> Self {
        Self::of_str(&format!("{value:#?}"))
    }

    pub fn of_str(value: &str) -> Self {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in value.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Self(hash)
    }

    pub fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactTables {
    pub sections: ArtifactSectionBuffer,
    pub host_dependencies: HostDependencyTable,
    pub source_files: SourceFileTable,
    pub debug_names: DebugNameTable,
}

impl ArtifactTables {
    pub fn from_module(module: &BytecodeModule) -> Self {
        let mut sections = Vec::new();
        push_section(&mut sections, ArtifactSectionId::Header, 1usize);
        push_section(
            &mut sections,
            ArtifactSectionId::Module,
            module.functions.len(),
        );
        push_section(
            &mut sections,
            ArtifactSectionId::Constants,
            module.constants.len(),
        );
        push_section(&mut sections, ArtifactSectionId::Types, module.types.len());
        push_section(
            &mut sections,
            ArtifactSectionId::Functions,
            module.function_table.len(),
        );
        push_section(
            &mut sections,
            ArtifactSectionId::PublicItems,
            module.public_items.len(),
        );
        push_section(
            &mut sections,
            ArtifactSectionId::ModuleSlots,
            module.module_slots.len(),
        );
        push_section(
            &mut sections,
            ArtifactSectionId::Fields,
            module.fields.len(),
        );
        push_section(&mut sections, ArtifactSectionId::Paths, module.paths.len());
        push_section(&mut sections, ArtifactSectionId::HostDependencies, 0usize);
        push_section(&mut sections, ArtifactSectionId::StringTable, 0usize);
        push_section(&mut sections, ArtifactSectionId::SourceFiles, 0usize);
        push_section(&mut sections, ArtifactSectionId::DebugNames, 0usize);
        push_section(
            &mut sections,
            ArtifactSectionId::Verification,
            module.functions.len(),
        );

        Self {
            sections,
            host_dependencies: Vec::new(),
            source_files: Vec::new(),
            debug_names: Vec::new(),
        }
    }
}

fn push_section(sections: &mut Vec<ArtifactSection>, id: ArtifactSectionId, record_count: usize) {
    sections.push(ArtifactSection {
        id,
        record_count,
        fingerprint: ArtifactFingerprint::of_debug(&(id, record_count)),
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactSectionId {
    Header,
    Module,
    Constants,
    Types,
    Functions,
    PublicItems,
    ModuleSlots,
    Fields,
    Paths,
    HostDependencies,
    StringTable,
    SourceFiles,
    DebugNames,
    Verification,
    Debug,
    Signatures,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSection {
    pub id: ArtifactSectionId,
    pub record_count: usize,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationMetadata {
    pub bytecode_verified: bool,
    pub function_layouts: FunctionLayoutBuffer,
    pub function_effects: FunctionEffectBuffer,
    pub control_flow_targets: ControlFlowTargetMetadataBuffer,
    pub typed_path_fingerprints: PathFingerprintBuffer,
    pub public_abi_fingerprints: PublicAbiFingerprintBuffer,
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    pub host_registry_fingerprint: ArtifactFingerprint,
    pub security_profile_requirements: Vec<String>,
    pub loader: LoaderValidationMetadata,
}

impl VerificationMetadata {
    pub fn from_module(module: &BytecodeModule, options: &ArtifactBuildOptions) -> Self {
        let bytecode_verified = verify_module(module).is_ok();
        let typed_path_fingerprints = module
            .paths
            .iter()
            .map(|path| PathDescriptorFingerprint {
                path: path.id,
                fingerprint: ArtifactFingerprint::of_debug(path),
            })
            .collect::<Vec<_>>();
        let public_abi_fingerprints = module
            .public_items
            .iter()
            .map(|item| PublicAbiFingerprint {
                name: item.fingerprint_name(),
                fingerprint: ArtifactFingerprint::of_debug(item),
            })
            .collect::<Vec<_>>();

        Self {
            bytecode_verified,
            function_layouts: module
                .functions
                .iter()
                .map(|function| FunctionLayoutMetadata {
                    function: function.id,
                    params: function.metadata.params.clone(),
                    return_type: function.metadata.return_type,
                    locals: function.metadata.locals.clone(),
                    registers: function.metadata.registers.clone(),
                })
                .collect(),
            function_effects: module
                .functions
                .iter()
                .map(|function| FunctionEffectMetadata {
                    function: function.id,
                    effects: function.metadata.effects,
                })
                .collect(),
            control_flow_targets: module
                .functions
                .iter()
                .map(|function| ControlFlowTargetMetadata {
                    function: function.id,
                    targets: function.metadata.control_flow_targets.clone(),
                })
                .collect(),
            typed_path_fingerprints: typed_path_fingerprints.clone(),
            public_abi_fingerprints: public_abi_fingerprints.clone(),
            dependency_fingerprints: options.dependency_fingerprints.clone(),
            host_registry_fingerprint: options.host_registry_fingerprint,
            security_profile_requirements: options.security_profile.clone().into_iter().collect(),
            loader: LoaderValidationMetadata {
                module_identity: options.module_identity.clone(),
                runtime_abi_version: options.runtime_abi_version.clone(),
                runtime_helper_abi_version: options.runtime_helper_abi_version.clone(),
                dependency_fingerprints: options.dependency_fingerprints.clone(),
                host_registry_fingerprint: options.host_registry_fingerprint,
                typed_path_fingerprints,
                public_abi_fingerprints,
                security_profile: options.security_profile.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionLayoutMetadata {
    pub function: FunctionRef,
    pub params: Vec<ValueType>,
    pub return_type: ValueType,
    pub locals: Vec<ValueType>,
    pub registers: Vec<ValueType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionEffectMetadata {
    pub function: FunctionRef,
    pub effects: crate::module::EffectSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowTargetMetadata {
    pub function: FunctionRef,
    pub targets: Vec<crate::bytecode::JumpTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDescriptorFingerprint {
    pub path: PathId,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicAbiFingerprint {
    pub name: String,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyFingerprint {
    pub module_id: String,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderValidationMetadata {
    pub module_identity: ArtifactModuleIdentity,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    pub host_registry_fingerprint: ArtifactFingerprint,
    pub typed_path_fingerprints: PathFingerprintBuffer,
    pub public_abi_fingerprints: PublicAbiFingerprintBuffer,
    pub security_profile: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtifactBuildOptions {
    pub module_identity: ArtifactModuleIdentity,
    pub module_epoch: Option<ModuleEpoch>,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    pub host_registry_fingerprint: ArtifactFingerprint,
    pub security_profile: Option<String>,
    pub debug: Option<DebugMetadata>,
    pub signatures: Option<ArtifactSignatures>,
}

impl Default for ArtifactModuleIdentity {
    fn default() -> Self {
        Self::single_file("memory://main.kg")
    }
}

impl Default for ArtifactBuildOptions {
    fn default() -> Self {
        Self {
            module_identity: ArtifactModuleIdentity::default(),
            module_epoch: None,
            runtime_abi_version: KAGARI_RUNTIME_ABI_VERSION.to_owned(),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.to_owned(),
            dependency_fingerprints: Vec::new(),
            host_registry_fingerprint: ArtifactFingerprint::empty(),
            security_profile: None,
            debug: None,
            signatures: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCompatibility {
    pub format_version: u16,
    pub language_version: String,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub module_identity: Option<ArtifactModuleIdentity>,
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    pub host_registry_fingerprint: ArtifactFingerprint,
    pub security_profile: Option<String>,
}

impl Default for ArtifactCompatibility {
    fn default() -> Self {
        Self {
            format_version: KBC_ARTIFACT_FORMAT_VERSION,
            language_version: KAGARI_LANGUAGE_VERSION.to_owned(),
            runtime_abi_version: KAGARI_RUNTIME_ABI_VERSION.to_owned(),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.to_owned(),
            module_identity: None,
            dependency_fingerprints: Vec::new(),
            host_registry_fingerprint: ArtifactFingerprint::empty(),
            security_profile: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugMetadata {
    pub stripped: bool,
    pub source_files: SourceFileTable,
    pub debug_names: DebugNameTable,
    pub functions: Vec<BytecodeDebugMetadata>,
}

impl DebugMetadata {
    pub fn from_module(module: &BytecodeModule) -> Self {
        Self {
            stripped: false,
            source_files: Vec::new(),
            debug_names: module
                .functions
                .iter()
                .map(|function| function.name.clone())
                .collect(),
            functions: module
                .functions
                .iter()
                .map(|function| function.metadata.debug.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSignatures {
    pub signatures: Vec<ArtifactSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSignature {
    pub key_id: String,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactValidationError {
    InvalidMagic([u8; 4]),
    FormatVersionMismatch {
        expected: u16,
        found: u16,
    },
    LanguageVersionMismatch {
        expected: String,
        found: String,
    },
    RuntimeAbiMismatch {
        expected: String,
        found: String,
    },
    RuntimeHelperAbiMismatch {
        expected: String,
        found: String,
    },
    ModuleIdentityMismatch {
        expected: ArtifactModuleIdentity,
        found: ArtifactModuleIdentity,
    },
    ContentHashMismatch,
    UnverifiedBytecode,
    DependencyFingerprintMismatch,
    HostRegistryFingerprintMismatch {
        expected: ArtifactFingerprint,
        found: ArtifactFingerprint,
    },
    SecurityProfileMismatch {
        expected: Option<String>,
        found: Option<String>,
    },
    PathFingerprintMismatch,
    PublicAbiFingerprintMismatch,
    Bytecode(crate::bytecode::BytecodeVerificationError),
}

pub type ArtifactSectionBuffer = Vec<ArtifactSection>;
pub type HostDependencyTable = Vec<String>;
pub type SourceFileTable = Vec<String>;
pub type DebugNameTable = Vec<String>;
pub type FunctionLayoutBuffer = Vec<FunctionLayoutMetadata>;
pub type FunctionEffectBuffer = Vec<FunctionEffectMetadata>;
pub type ControlFlowTargetMetadataBuffer = Vec<ControlFlowTargetMetadata>;
pub type PathFingerprintBuffer = Vec<PathDescriptorFingerprint>;
pub type PublicAbiFingerprintBuffer = Vec<PublicAbiFingerprint>;
pub type DependencyFingerprintBuffer = Vec<DependencyFingerprint>;
