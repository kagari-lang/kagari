use bincode::Options;
use kagari_common::identity::ModuleIdentity;
use std::fmt::{self, Display, Formatter};

use crate::{
    bytecode::{
        BytecodeDebugMetadata, BytecodeModule, BytecodeProgram, FunctionRef, PathId, verify_program,
    },
    module::ValueType,
};
use serde::{Deserialize, Serialize};

pub const KBC_MAGIC: [u8; 4] = *b"KBC\0";
pub const KBC_ARTIFACT_FORMAT_VERSION: u16 = 10;
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .with_limit(MAX_ARTIFACT_BYTES)
}
pub const KAGARI_LANGUAGE_VERSION: &str = "kagari-language-v1";
pub const KAGARI_COMPILER_FINGERPRINT: &str = concat!("kagari-ir/", env!("CARGO_PKG_VERSION"));
pub const KAGARI_RUNTIME_ABI_VERSION: &str = "kagari-runtime-abi-v5";
pub const KAGARI_RUNTIME_HELPER_ABI_VERSION: &str = "kagari-runtime-helper-abi-v3";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbcArtifact {
    pub header: ArtifactHeader,
    pub program: BytecodeProgram,
    pub tables: ArtifactTables,
    pub verification: VerificationMetadata,
    pub debug: Option<DebugMetadata>,
    pub signatures: Option<ArtifactSignatures>,
}

impl KbcArtifact {
    pub fn from_program(program: BytecodeProgram, options: ArtifactBuildOptions) -> Self {
        let fallback = BytecodeModule::default();
        let module = program
            .modules
            .get(program.root.index())
            .unwrap_or(&fallback);
        let mut tables = ArtifactTables::from_program(&program);
        let verification = VerificationMetadata::from_program(&program, &options);
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
                module_identity: module.identity.clone(),
                module_epoch: options.module_epoch,
                content_hash: ArtifactFingerprint::empty(),
            },
            program,
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
        verify_program(&self.program).map_err(ArtifactValidationError::Bytecode)?;
        let module = &self.program.modules[self.program.root.index()];
        if let Some(expected_module) = &requirements.module_identity
            && &self.header.module_identity != expected_module
        {
            return Err(ArtifactValidationError::ModuleIdentityMismatch {
                expected: Box::new(expected_module.clone()),
                found: Box::new(self.header.module_identity.clone()),
            });
        }
        if self.verification.loader.module_identity != self.header.module_identity {
            return Err(ArtifactValidationError::ModuleIdentityMismatch {
                expected: Box::new(self.header.module_identity.clone()),
                found: Box::new(self.verification.loader.module_identity.clone()),
            });
        }
        if module.identity != self.header.module_identity {
            return Err(ArtifactValidationError::ModuleIdentityMismatch {
                expected: Box::new(self.header.module_identity.clone()),
                found: Box::new(module.identity.clone()),
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

        if !self.verification.bytecode_verified {
            return Err(ArtifactValidationError::UnverifiedBytecode);
        }
        if self.verification.dependency_fingerprints != self.program.dependency_fingerprints()
            || self.verification.loader.dependency_fingerprints
                != self.verification.dependency_fingerprints
            || requirements
                .dependency_fingerprints
                .as_ref()
                .is_some_and(|required| required != &self.verification.dependency_fingerprints)
        {
            return Err(ArtifactValidationError::DependencyFingerprintMismatch);
        }
        if self.verification.host_interface_fingerprint
            != ArtifactFingerprint::of_program_hosts(&self.program)
        {
            return Err(ArtifactValidationError::HostInterfaceFingerprintMismatch {
                expected: ArtifactFingerprint::of_program_hosts(&self.program),
                found: self.verification.host_interface_fingerprint,
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
        let derived = VerificationMetadata::build(
            &self.program,
            &ArtifactBuildOptions {
                runtime_abi_version: self.header.runtime_abi_version.clone(),
                runtime_helper_abi_version: self.header.runtime_helper_abi_version.clone(),
                security_profile: self.verification.loader.security_profile.clone(),
                ..Default::default()
            },
            true,
        );
        if self.verification != derived {
            return Err(ArtifactValidationError::VerificationMetadataMismatch);
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, ArtifactCodecError> {
        codec().serialize(self).map_err(ArtifactCodecError::from)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ArtifactCodecError> {
        if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
            return Err(ArtifactCodecError {
                message: "artifact exceeds size limit".into(),
            });
        }
        if bytes.get(..4) != Some(KBC_MAGIC.as_slice())
            || bytes.get(4..6) != Some(KBC_ARTIFACT_FORMAT_VERSION.to_le_bytes().as_slice())
        {
            return Err(ArtifactCodecError {
                message: "unsupported artifact magic or format version".into(),
            });
        }
        codec().deserialize(bytes).map_err(ArtifactCodecError::from)
    }

    fn validate_header(
        &self,
        requirements: &ArtifactCompatibility,
    ) -> Result<(), ArtifactValidationError> {
        if self.header.magic != KBC_MAGIC {
            return Err(ArtifactValidationError::InvalidMagic(self.header.magic));
        }
        if self.header.format_version != KBC_ARTIFACT_FORMAT_VERSION
            || self.header.format_version != requirements.format_version
        {
            return Err(ArtifactValidationError::FormatVersionMismatch {
                expected: KBC_ARTIFACT_FORMAT_VERSION,
                found: self.header.format_version,
            });
        }
        if self.header.language_version != KAGARI_LANGUAGE_VERSION
            || self.header.language_version != requirements.language_version
        {
            return Err(ArtifactValidationError::LanguageVersionMismatch {
                expected: KAGARI_LANGUAGE_VERSION.to_owned(),
                found: self.header.language_version.clone(),
            });
        }
        if self.header.runtime_abi_version != KAGARI_RUNTIME_ABI_VERSION
            || self.header.runtime_abi_version != requirements.runtime_abi_version
        {
            return Err(ArtifactValidationError::RuntimeAbiMismatch {
                expected: requirements.runtime_abi_version.clone(),
                found: self.header.runtime_abi_version.clone(),
            });
        }
        if self.header.runtime_helper_abi_version != KAGARI_RUNTIME_HELPER_ABI_VERSION
            || self.header.runtime_helper_abi_version != requirements.runtime_helper_abi_version
        {
            return Err(ArtifactValidationError::RuntimeHelperAbiMismatch {
                expected: requirements.runtime_helper_abi_version.clone(),
                found: self.header.runtime_helper_abi_version.clone(),
            });
        }
        Ok(())
    }

    fn compute_content_hash(&self) -> ArtifactFingerprint {
        let mut header = self.header.clone();
        header.content_hash = ArtifactFingerprint::empty();
        ArtifactFingerprint::of_serialized(&(
            &header,
            &self.program,
            &self.tables,
            &self.verification,
            &self.debug,
            &self.signatures,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactHeader {
    pub magic: [u8; 4],
    pub format_version: u16,
    pub language_version: String,
    pub compiler_fingerprint: String,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub encoding: ArtifactEncoding,
    pub module_identity: ModuleIdentity,
    pub module_epoch: Option<ModuleEpoch>,
    pub content_hash: ArtifactFingerprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactEncoding {
    CanonicalLittleEndian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleEpoch(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactFingerprint(pub u64);

impl ArtifactFingerprint {
    /// Canonical required ABI set; documentation and import slot order do not affect it.
    pub fn of_host_interface(interface: &kagari_common::host_interface::HostInterface) -> Self {
        let mut functions = interface.functions.clone();
        for function in &mut functions {
            function.documentation.clear();
        }
        functions.sort_by(|a, b| a.id.cmp(&b.id));
        Self::of_serialized(&("kagari-required-host-interface-v1", functions))
    }

    pub fn of_program_hosts(program: &BytecodeProgram) -> Self {
        Self::of_serialized(&(
            "kagari-program-hosts-v1",
            program
                .modules
                .iter()
                .map(|module| {
                    (
                        &module.identity,
                        Self::of_host_interface(&module.host_interface),
                    )
                })
                .collect::<Vec<_>>(),
        ))
    }
    pub fn empty() -> Self {
        Self(0)
    }

    /// FNV-1a/64 over domain-separated fixed-width little-endian bincode v1.
    /// This is a compatibility fingerprint, not authentication or a signature.
    pub fn of_serialized(value: &impl Serialize) -> Self {
        struct Sink(u64);
        impl std::io::Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                for byte in bytes {
                    self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Sink(Self::of_str("kagari-canonical-v2").0);
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize_into(&mut sink, value)
            .expect("canonical metadata serialization must succeed");
        Self(sink.0)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactTables {
    pub sections: ArtifactSectionBuffer,
    pub source_files: SourceFileTable,
    pub debug_names: DebugNameTable,
}

impl ArtifactTables {
    pub fn from_program(program: &BytecodeProgram) -> Self {
        let mut sections = Vec::new();
        let count = |f: fn(&BytecodeModule) -> usize| program.modules.iter().map(f).sum::<usize>();
        for (id, records) in [
            (ArtifactSectionId::Header, 1),
            (ArtifactSectionId::Module, program.modules.len()),
            (ArtifactSectionId::Constants, count(|m| m.constants.len())),
            (ArtifactSectionId::Types, count(|m| m.types.len())),
            (ArtifactSectionId::Functions, count(|m| m.functions.len())),
            (
                ArtifactSectionId::PublicItems,
                count(|m| m.public_items.len()),
            ),
            (
                ArtifactSectionId::ModuleSlots,
                count(|m| m.module_slots.len()),
            ),
            (
                ArtifactSectionId::StructLayouts,
                count(|m| m.structures.len()),
            ),
            (ArtifactSectionId::Paths, count(|m| m.paths.len())),
            (
                ArtifactSectionId::HostDependencies,
                count(|m| m.host_interface.functions.len()),
            ),
            (ArtifactSectionId::SourceFiles, program.modules.len()),
            (
                ArtifactSectionId::Verification,
                count(|m| m.functions.len()),
            ),
        ] {
            push_section(&mut sections, id, records);
        }
        Self {
            sections,
            source_files: program
                .modules
                .iter()
                .map(|module| module.source_name.clone())
                .collect(),
            debug_names: Vec::new(),
        }
    }
}

fn push_section(sections: &mut Vec<ArtifactSection>, id: ArtifactSectionId, record_count: usize) {
    sections.push(ArtifactSection {
        id,
        record_count,
        fingerprint: ArtifactFingerprint::of_serialized(&(id, record_count)),
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArtifactSectionId {
    Header,
    Module,
    Constants,
    Types,
    Functions,
    PublicItems,
    ModuleSlots,
    StructLayouts,
    Paths,
    HostDependencies,
    StringTable,
    SourceFiles,
    DebugNames,
    Verification,
    Debug,
    Signatures,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSection {
    pub id: ArtifactSectionId,
    pub record_count: usize,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationMetadata {
    pub bytecode_verified: bool,
    /// Root-member summaries. Dependency metadata remains in its BytecodeModule;
    /// all members are verified before these derived summaries are accepted.
    pub function_layouts: FunctionLayoutBuffer,
    pub function_effects: FunctionEffectBuffer,
    pub control_flow_targets: ControlFlowTargetMetadataBuffer,
    pub typed_path_fingerprints: PathFingerprintBuffer,
    pub public_abi_fingerprints: PublicAbiFingerprintBuffer,
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    pub host_interface_fingerprint: ArtifactFingerprint,
    pub security_profile_requirements: Vec<String>,
    pub loader: LoaderValidationMetadata,
}

impl VerificationMetadata {
    pub fn from_program(program: &BytecodeProgram, options: &ArtifactBuildOptions) -> Self {
        Self::build(program, options, verify_program(program).is_ok())
    }
    fn build(
        program: &BytecodeProgram,
        options: &ArtifactBuildOptions,
        bytecode_verified: bool,
    ) -> Self {
        let fallback = BytecodeModule::default();
        let module = program
            .modules
            .get(program.root.index())
            .unwrap_or(&fallback);
        let typed_path_fingerprints = module
            .paths
            .iter()
            .map(|path| PathDescriptorFingerprint {
                path: path.id,
                fingerprint: ArtifactFingerprint::of_serialized(path),
            })
            .collect::<Vec<_>>();
        let public_abi_fingerprints = module
            .public_items
            .iter()
            .map(|item| PublicAbiFingerprint {
                name: item.fingerprint_name(),
                fingerprint: ArtifactFingerprint::of_serialized(item),
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
            dependency_fingerprints: program.dependency_fingerprints(),
            host_interface_fingerprint: ArtifactFingerprint::of_program_hosts(program),
            security_profile_requirements: options.security_profile.clone().into_iter().collect(),
            loader: LoaderValidationMetadata {
                module_identity: module.identity.clone(),
                runtime_abi_version: options.runtime_abi_version.clone(),
                runtime_helper_abi_version: options.runtime_helper_abi_version.clone(),
                dependency_fingerprints: program.dependency_fingerprints(),
                typed_path_fingerprints,
                public_abi_fingerprints,
                security_profile: options.security_profile.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionLayoutMetadata {
    pub function: FunctionRef,
    pub params: Vec<ValueType>,
    pub return_type: ValueType,
    pub locals: Vec<ValueType>,
    pub registers: Vec<ValueType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionEffectMetadata {
    pub function: FunctionRef,
    pub effects: crate::module::EffectSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlFlowTargetMetadata {
    pub function: FunctionRef,
    pub targets: Vec<crate::bytecode::JumpTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathDescriptorFingerprint {
    pub path: PathId,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicAbiFingerprint {
    pub name: String,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyFingerprint {
    pub module_id: ModuleIdentity,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoaderValidationMetadata {
    pub module_identity: ModuleIdentity,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    pub typed_path_fingerprints: PathFingerprintBuffer,
    pub public_abi_fingerprints: PublicAbiFingerprintBuffer,
    pub security_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactBuildOptions {
    pub module_epoch: Option<ModuleEpoch>,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub security_profile: Option<String>,
    pub debug: Option<DebugMetadata>,
    pub signatures: Option<ArtifactSignatures>,
}

impl Default for ArtifactBuildOptions {
    fn default() -> Self {
        Self {
            module_epoch: None,
            runtime_abi_version: KAGARI_RUNTIME_ABI_VERSION.to_owned(),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.to_owned(),
            security_profile: None,
            debug: None,
            signatures: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactCompatibility {
    pub format_version: u16,
    pub language_version: String,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub module_identity: Option<ModuleIdentity>,
    pub dependency_fingerprints: Option<DependencyFingerprintBuffer>,
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
            dependency_fingerprints: None,
            security_profile: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSignatures {
    pub signatures: Vec<ArtifactSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        expected: Box<ModuleIdentity>,
        found: Box<ModuleIdentity>,
    },
    ContentHashMismatch,
    UnverifiedBytecode,
    VerificationMetadataMismatch,
    DependencyFingerprintMismatch,
    HostInterfaceFingerprintMismatch {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCodecError {
    message: String,
}

impl ArtifactCodecError {
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<Box<bincode::ErrorKind>> for ArtifactCodecError {
    fn from(error: Box<bincode::ErrorKind>) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl Display for ArtifactCodecError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "artifact codec error: {}", self.message)
    }
}

impl std::error::Error for ArtifactCodecError {}

impl ArtifactValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidMagic(_) => "KG_ARTIFACT_INVALID_MAGIC",
            Self::FormatVersionMismatch { .. } => "KG_ARTIFACT_FORMAT_VERSION_MISMATCH",
            Self::LanguageVersionMismatch { .. } => "KG_ARTIFACT_LANGUAGE_VERSION_MISMATCH",
            Self::RuntimeAbiMismatch { .. } => "KG_ARTIFACT_RUNTIME_ABI_MISMATCH",
            Self::RuntimeHelperAbiMismatch { .. } => "KG_ARTIFACT_RUNTIME_HELPER_ABI_MISMATCH",
            Self::ModuleIdentityMismatch { .. } => "KG_ARTIFACT_MODULE_IDENTITY_MISMATCH",
            Self::ContentHashMismatch => "KG_ARTIFACT_CONTENT_HASH_MISMATCH",
            Self::UnverifiedBytecode => "KG_ARTIFACT_UNVERIFIED_BYTECODE",
            Self::VerificationMetadataMismatch => "KG_ARTIFACT_VERIFICATION_METADATA_MISMATCH",
            Self::DependencyFingerprintMismatch => "KG_ARTIFACT_DEPENDENCY_FINGERPRINT_MISMATCH",
            Self::HostInterfaceFingerprintMismatch { .. } => {
                "KG_ARTIFACT_HOST_INTERFACE_FINGERPRINT_MISMATCH"
            }
            Self::SecurityProfileMismatch { .. } => "KG_ARTIFACT_SECURITY_PROFILE_MISMATCH",
            Self::PathFingerprintMismatch => "KG_ARTIFACT_PATH_FINGERPRINT_MISMATCH",
            Self::PublicAbiFingerprintMismatch => "KG_ARTIFACT_PUBLIC_ABI_FINGERPRINT_MISMATCH",
            Self::Bytecode(error) => error.code(),
        }
    }
}

impl Display for ArtifactValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic(magic) => write!(f, "invalid KBC magic bytes {magic:?}"),
            Self::FormatVersionMismatch { expected, found } => write!(
                f,
                "artifact format version mismatch: expected {expected}, found {found}"
            ),
            Self::LanguageVersionMismatch { expected, found } => write!(
                f,
                "artifact language version mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::RuntimeAbiMismatch { expected, found } => write!(
                f,
                "artifact runtime ABI mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::RuntimeHelperAbiMismatch { expected, found } => write!(
                f,
                "artifact runtime helper ABI mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::ModuleIdentityMismatch { expected, found } => write!(
                f,
                "artifact module identity mismatch: expected `{}`, found `{}`",
                expected, found
            ),
            Self::ContentHashMismatch => write!(f, "artifact content hash mismatch"),
            Self::UnverifiedBytecode => write!(f, "artifact bytecode was not verified"),
            Self::VerificationMetadataMismatch => {
                write!(f, "artifact verification metadata differs from its program")
            }
            Self::DependencyFingerprintMismatch => {
                write!(f, "artifact dependency fingerprints mismatch")
            }
            Self::HostInterfaceFingerprintMismatch { expected, found } => write!(
                f,
                "artifact host interface fingerprint mismatch: expected {}, found {}",
                expected.to_hex(),
                found.to_hex()
            ),
            Self::SecurityProfileMismatch { expected, found } => write!(
                f,
                "artifact security profile mismatch: expected {expected:?}, found {found:?}"
            ),
            Self::PathFingerprintMismatch => {
                write!(f, "artifact typed path fingerprints mismatch")
            }
            Self::PublicAbiFingerprintMismatch => {
                write!(f, "artifact public ABI fingerprints mismatch")
            }
            Self::Bytecode(error) => write!(f, "artifact bytecode verification failed: {error}"),
        }
    }
}

impl std::error::Error for ArtifactValidationError {}

pub type ArtifactSectionBuffer = Vec<ArtifactSection>;
pub type SourceFileTable = Vec<String>;
pub type DebugNameTable = Vec<String>;
pub type FunctionLayoutBuffer = Vec<FunctionLayoutMetadata>;
pub type FunctionEffectBuffer = Vec<FunctionEffectMetadata>;
pub type ControlFlowTargetMetadataBuffer = Vec<ControlFlowTargetMetadata>;
pub type PathFingerprintBuffer = Vec<PathDescriptorFingerprint>;
pub type PublicAbiFingerprintBuffer = Vec<PublicAbiFingerprint>;
pub type DependencyFingerprintBuffer = Vec<DependencyFingerprint>;

#[cfg(test)]
mod canonical_tests {
    use super::*;

    #[test]
    fn bytecode_cannot_claim_a_different_identity_from_its_header() {
        let mut artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            Default::default(),
        );
        artifact.program.modules[artifact.program.root.index()].identity =
            ModuleIdentity::single_file("forged.kgr");
        artifact.header.content_hash = artifact.compute_content_hash();
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ModuleIdentityMismatch { .. })
        ));
    }

    #[test]
    fn legacy_language_semantics_cannot_be_opted_into() {
        let mut artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            Default::default(),
        );
        artifact.header.language_version = "0.1.0".into();
        let requirements = ArtifactCompatibility {
            language_version: "0.1.0".into(),
            ..Default::default()
        };
        assert!(matches!(
            artifact.validate_for_loader(&requirements),
            Err(ArtifactValidationError::LanguageVersionMismatch { .. })
        ));
    }

    #[test]
    fn fingerprints_depend_on_serialized_values_not_rust_debug_names() {
        #[derive(Debug, Serialize)]
        struct First(u32);
        #[derive(Debug, Serialize)]
        struct Renamed(u32);
        assert_eq!(
            ArtifactFingerprint::of_serialized(&First(42)),
            ArtifactFingerprint::of_serialized(&Renamed(42))
        );
        assert_ne!(
            ArtifactFingerprint::of_serialized(&First(42)),
            ArtifactFingerprint::of_serialized(&First(43))
        );
    }

    #[test]
    fn decoder_rejects_old_versions_trailing_bytes_and_oversized_lengths() {
        let artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            ArtifactBuildOptions::default(),
        );
        let bytes = artifact.to_bytes().unwrap();
        assert!(KbcArtifact::from_bytes(&bytes).is_ok());
        for version in 1..KBC_ARTIFACT_FORMAT_VERSION {
            let mut old = bytes.clone();
            old[4..6].copy_from_slice(&version.to_le_bytes());
            assert!(KbcArtifact::from_bytes(&old).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(KbcArtifact::from_bytes(&trailing).is_err());
        let mut enormous_string = bytes;
        enormous_string[6..14].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(KbcArtifact::from_bytes(&enormous_string).is_err());
    }

    #[test]
    fn required_host_fingerprint_is_derived_and_independent_of_docs_and_order() {
        use kagari_common::host_interface::{
            HostFunctionDeclaration, HostInterface, HostValueType, standard_log,
        };
        let interface = HostInterface {
            functions: vec![
                standard_log(),
                HostFunctionDeclaration::new("host.other", vec![], HostValueType::Unit),
            ],
        };
        let fingerprint = ArtifactFingerprint::of_host_interface(&interface);
        let mut reordered = interface.clone();
        reordered.functions.reverse();
        reordered.functions[0].documentation = "different docs".into();
        assert_eq!(
            fingerprint,
            ArtifactFingerprint::of_host_interface(&reordered)
        );
        reordered.functions[0].effects.may_trap = true;
        assert_ne!(
            fingerprint,
            ArtifactFingerprint::of_host_interface(&reordered)
        );
        let mut artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    host_interface: interface,
                    ..Default::default()
                }],
            },
            ArtifactBuildOptions::default(),
        );
        assert_eq!(
            artifact.verification.host_interface_fingerprint,
            ArtifactFingerprint::of_program_hosts(&artifact.program)
        );
        artifact.verification.host_interface_fingerprint = ArtifactFingerprint::empty();
        artifact.header.content_hash = artifact.compute_content_hash();
        assert!(matches!(
            artifact.validate_for_loader(&ArtifactCompatibility::default()),
            Err(ArtifactValidationError::HostInterfaceFingerprintMismatch { .. })
        ));
    }

    #[test]
    fn header_metadata_is_covered_and_old_format_cannot_be_opted_into() {
        let mut artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            ArtifactBuildOptions::default(),
        );
        artifact
            .header
            .module_identity
            .package
            .0
            .push_str("changed");
        assert!(matches!(
            artifact.validate_for_loader(&ArtifactCompatibility::default()),
            Err(ArtifactValidationError::ContentHashMismatch)
        ));
        artifact.header.format_version = 1;
        let requirements = ArtifactCompatibility {
            format_version: 1,
            ..ArtifactCompatibility::default()
        };
        assert!(matches!(
            artifact.validate_for_loader(&requirements),
            Err(ArtifactValidationError::FormatVersionMismatch { .. })
        ));
    }

    #[test]
    fn recomputed_checksum_cannot_hide_dependency_or_verification_metadata_changes() {
        use crate::bytecode::ModuleRef;
        let program = BytecodeProgram {
            root: ModuleRef::new(1),
            modules: vec![
                BytecodeModule {
                    identity: ModuleIdentity::single_file("dependency"),
                    ..Default::default()
                },
                BytecodeModule {
                    identity: ModuleIdentity::single_file("root"),
                    dependencies: vec![ModuleRef::new(0)],
                    ..Default::default()
                },
            ],
        };
        let original = KbcArtifact::from_program(program, Default::default());
        original.validate_for_loader(&Default::default()).unwrap();
        let mut artifact = original.clone();
        artifact.program.modules[0].source_name.push_str("changed");
        artifact.header.content_hash = artifact.compute_content_hash();
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::DependencyFingerprintMismatch)
        ));
        let mut artifact = original;
        artifact
            .verification
            .public_abi_fingerprints
            .push(PublicAbiFingerprint {
                name: "invented".into(),
                fingerprint: ArtifactFingerprint::empty(),
            });
        artifact.verification.loader.public_abi_fingerprints =
            artifact.verification.public_abi_fingerprints.clone();
        artifact.header.content_hash = artifact.compute_content_hash();
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::VerificationMetadataMismatch)
        ));
    }
}
