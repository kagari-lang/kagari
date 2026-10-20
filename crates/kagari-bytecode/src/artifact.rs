use crate::module::BytecodeSemanticSlots;
use crate::{
    artifact::limits::{artifact_count_limit, metadata_count_limit, program_count_limit},
    instruction::{JumpTarget, PathId},
    module::{BytecodeDebugMetadata, BytecodeModule, RootSlotLayout},
    native_input::PortableMir,
    program::{
        BytecodeProgram, identity,
        verified::{ProgramVerification, VerifiedBytecodeProgram},
        verify_graph,
    },
    verifier::BytecodeVerificationError,
};
use bincode::{DefaultOptions, ErrorKind, Options};
use kagari_abi::{
    representation::ValueType,
    version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata, scoped::DefinitionReferences, table::DefinitionTable,
    },
};
use kagari_contract::{
    decode_limits::{
        MAX_FUNCTIONS, MAX_INSTRUCTIONS, MAX_MODULES, MAX_NESTED_RECORDS, MAX_TABLE_RECORDS,
    },
    effects::EffectSet,
    ids::FunctionRef,
    types::{PublicItem, wire::PublicItemRecord},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io, io::Write};

mod limits;
mod wire;

pub const KBC_MAGIC: [u8; 4] = *b"KBC\0";
pub const KBC_ARTIFACT_FORMAT_VERSION: u16 = 116;
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_ARTIFACT_MODULES: usize = MAX_MODULES;
pub const MAX_ARTIFACT_FUNCTIONS: usize = MAX_FUNCTIONS;
pub const MAX_ARTIFACT_INSTRUCTIONS: usize = MAX_INSTRUCTIONS;
pub const MAX_ARTIFACT_TABLE_RECORDS: usize = MAX_TABLE_RECORDS;
pub const MAX_ARTIFACT_NESTED_RECORDS: usize = MAX_NESTED_RECORDS;

fn codec() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .with_limit(MAX_ARTIFACT_BYTES)
}

fn exceeds_encoded_size(value: &impl Serialize) -> bool {
    matches!(
        codec().serialized_size(value),
        Err(error) if matches!(*error, bincode::ErrorKind::SizeLimit)
    )
}

/// Apply the artifact's in-memory resource budget before hashing or linking code.
pub fn validate_program_resource_limits(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), ArtifactValidationError> {
    if let Some(reason) = program_count_limit(program, definitions) {
        return Err(ArtifactValidationError::ResourceLimit(reason));
    }
    let portable = wire::encode_program(program, definitions, cancel)?;
    if exceeds_encoded_size(&portable) {
        return Err(ArtifactValidationError::ResourceLimit(
            "artifact encoded size limit exceeded",
        ));
    }
    Ok(())
}

/// Encode complete scoped program facts for exact native-input comparison.
///
/// Canonical bytes preserve every field and floating-point payload while excluding
/// process-local identity numbers. This carries neither an artifact header nor
/// executable evidence; callers must validate the supplying products separately.
///
/// Returns scope, count/byte-limit, cancellation or encoding errors. No semantic
/// model instantiated with file references is exposed to the caller.
pub fn encode_program_facts(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, ArtifactCodecError> {
    cancel.check().map_err(|_| ArtifactCodecError {
        message: "program encoding cancelled".into(),
    })?;
    if let Some(reason) = program_count_limit(program, definitions) {
        return Err(ArtifactCodecError {
            message: reason.into(),
        });
    }
    let records =
        wire::encode_program(program, definitions, cancel).map_err(|error| ArtifactCodecError {
            message: error.to_string(),
        })?;
    let bytes = codec()
        .serialize(&records)
        .map_err(ArtifactCodecError::from)?;
    cancel.check().map_err(|_| ArtifactCodecError {
        message: "program encoding cancelled".into(),
    })?;
    Ok(bytes)
}

pub const KAGARI_LANGUAGE_VERSION: &str = "kagari-language-v3";
pub const KAGARI_COMPILER_FINGERPRINT: &str =
    concat!("kagari-compiler/", env!("CARGO_PKG_VERSION"));

/// Editable artifact facts with fixed scoped identities and untrusted summaries.
///
/// File conversion uses the private wire records through to_bytes/from_bytes.
/// Verification and runtime adoption must retain and validate the enclosing scope.
#[derive(Debug, Clone)]
pub struct KbcArtifact {
    pub header: ArtifactHeader,
    pub program: BytecodeProgram,
    pub tables: ArtifactTables,
    pub verification: VerificationMetadata,
    pub debug: Option<DebugMetadata>,
    pub signatures: Option<ArtifactSignatures>,
    pub portable_mir: Option<PortableMir>,
}

/// Executable contents retained after complete envelope and compatibility validation.
/// Portable MIR remains untrusted until checked against the sealed bytecode.
#[derive(Debug)]
pub struct VerifiedArtifact {
    bytecode: VerifiedBytecodeProgram,
    portable_mir: Option<PortableMir>,
}

impl VerifiedArtifact {
    pub fn bytecode(&self) -> &VerifiedBytecodeProgram {
        &self.bytecode
    }

    pub fn portable_mir(&self) -> Option<&PortableMir> {
        self.portable_mir.as_ref()
    }

    pub fn into_bytecode(self) -> VerifiedBytecodeProgram {
        self.bytecode
    }
}

impl KbcArtifact {
    /// Consume the open artifact so checked executable data cannot change between
    /// loader validation, native correspondence and runtime adoption.
    pub fn into_verified(
        self,
        requirements: &ArtifactCompatibility,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<VerifiedArtifact, ArtifactValidationError> {
        let verification = self.validate_execution(requirements, context, cancel)?;
        Ok(VerifiedArtifact {
            bytecode: VerifiedBytecodeProgram::from_verified(
                self.program,
                verification,
                context.snapshot(),
                cancel,
            )?,
            portable_mir: self.portable_mir,
        })
    }

    /// Validate editable executable records before constructing an export envelope.
    pub fn from_program(
        program: BytecodeProgram,
        options: ArtifactBuildOptions,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<Self, ArtifactValidationError> {
        let program = VerifiedBytecodeProgram::new(program, context, cancel)?;
        Self::from_verified(program, options, cancel).map(DefinitionMetadata::into_records)
    }

    /// Export an exact sealed program without repeating its executable proof.
    ///
    /// Export-only metadata, payload limits and canonical encoding remain checked.
    /// The resulting editable envelope carries identities but no execution seal;
    /// importing it requires independent validation even if it has not been edited.
    pub fn from_verified(
        program: VerifiedBytecodeProgram,
        options: ArtifactBuildOptions,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<Self>, ArtifactValidationError> {
        cancel
            .check()
            .map_err(|_| ArtifactValidationError::Identity(DefinitionMappingError::Cancelled))?;
        let (definitions, program) = program.into_unverified().into_parts();
        if options
            .portable_mir
            .as_ref()
            .is_some_and(|payload| payload.bytes.len() as u64 > MAX_ARTIFACT_BYTES)
        {
            return Err(ArtifactValidationError::ResourceLimit(
                "portable MIR byte limit exceeded",
            ));
        }
        if let Some(reason) = program_count_limit(&program, &definitions) {
            return Err(ArtifactValidationError::ResourceLimit(reason));
        }
        if let Some(reason) =
            metadata_count_limit(options.debug.as_ref(), options.signatures.as_ref())
        {
            return Err(ArtifactValidationError::ResourceLimit(reason));
        }
        if exceeds_encoded_size(&options) {
            return Err(ArtifactValidationError::ResourceLimit(
                "artifact encoded size limit exceeded",
            ));
        }
        let verification = VerificationMetadata::build(&program, &options, &definitions)?;
        let module = &program.modules[program.root.index()];
        let debug = options.debug;
        let signatures = options.signatures;
        let portable_mir = options.portable_mir;
        let tables = ArtifactTables::with_metadata(
            &program,
            debug.as_ref(),
            signatures.as_ref(),
            portable_mir.as_ref(),
        );
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
            portable_mir,
        };
        artifact.validate_resource_limits(&definitions, cancel)?;
        artifact.header.content_hash = artifact.compute_content_hash(&definitions)?;
        DefinitionMetadata::checked(definitions, artifact, cancel)
            .map_err(ArtifactValidationError::Identity)
    }

    /// Bound the complete editable envelope and check its reference scope.
    /// This does not establish compatibility, graph validity or executable evidence.
    pub fn validate_resource_limits(
        &self,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<(), ArtifactValidationError> {
        if let Some(reason) = artifact_count_limit(self, definitions) {
            return Err(ArtifactValidationError::ResourceLimit(reason));
        }
        // Check every envelope reference against the input prefix before verification
        // collects implicit declaration names in the append-only context.
        let portable = wire::encode_artifact(self, definitions, cancel)?;
        if exceeds_encoded_size(&portable) {
            return Err(ArtifactValidationError::ResourceLimit(
                "artifact encoded size limit exceeded",
            ));
        }
        Ok(())
    }

    pub fn validate_for_loader(
        &self,
        requirements: &ArtifactCompatibility,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<(), ArtifactValidationError> {
        self.validate_execution(requirements, context, cancel)
            .map(|_| ())
    }

    fn validate_execution(
        &self,
        requirements: &ArtifactCompatibility,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<ProgramVerification, ArtifactValidationError> {
        self.validate_resource_limits(&context.snapshot(), cancel)?;
        self.validate_header(requirements)?;
        let definitions = identity::collect(
            &self.program.modules.iter().collect::<Vec<_>>(),
            context,
            cancel,
        )
        .map_err(ArtifactValidationError::Bytecode)?;
        let verification = verify_graph(&self.program, &definitions, cancel)
            .map_err(ArtifactValidationError::Bytecode)?;
        if self.header.content_hash != self.compute_content_hash(&definitions)? {
            return Err(ArtifactValidationError::ContentHashMismatch);
        }
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
        if self.verification.dependency_fingerprints
            != self.program.dependency_fingerprints(&definitions)?
            || self.verification.loader.dependency_fingerprints
                != self.verification.dependency_fingerprints
            || requirements
                .dependency_fingerprints
                .as_ref()
                .is_some_and(|required| required != &self.verification.dependency_fingerprints)
        {
            return Err(ArtifactValidationError::DependencyFingerprintMismatch);
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

                ..Default::default()
            },
            &definitions,
        )?;
        if self.verification != derived {
            return Err(ArtifactValidationError::VerificationMetadataMismatch);
        }
        if self.tables
            != ArtifactTables::with_metadata(
                &self.program,
                self.debug.as_ref(),
                self.signatures.as_ref(),
                self.portable_mir.as_ref(),
            )
        {
            return Err(ArtifactValidationError::TableMismatch);
        }
        Ok(verification)
    }

    pub fn to_bytes(&self, definitions: &DefinitionTable) -> Result<Vec<u8>, ArtifactCodecError> {
        if let Some(reason) = artifact_count_limit(self, definitions) {
            return Err(ArtifactCodecError {
                message: reason.into(),
            });
        }
        let portable = wire::encode_artifact(self, definitions, &CancellationToken::default())
            .map_err(|error| ArtifactCodecError {
                message: error.to_string(),
            })?;
        if exceeds_encoded_size(&portable) {
            return Err(ArtifactCodecError {
                message: "artifact encoded size limit exceeded".into(),
            });
        }
        codec()
            .serialize(&portable)
            .map_err(ArtifactCodecError::from)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<DefinitionMetadata<Self>, ArtifactCodecError> {
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
        let portable: wire::ArtifactEnvelope = codec()
            .deserialize(bytes)
            .map_err(ArtifactCodecError::from)?;
        let cancel = CancellationToken::default();
        let artifact = portable
            .decode(&cancel)
            .map_err(|error| ArtifactCodecError {
                message: error.to_string(),
            })?;
        if let Some(reason) = artifact_count_limit(artifact.records(), artifact.definitions()) {
            return Err(ArtifactCodecError {
                message: reason.into(),
            });
        }
        Ok(artifact)
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

    fn compute_content_hash(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<ArtifactFingerprint, DefinitionMappingError> {
        let mut content = self.clone();
        content.header.content_hash = ArtifactFingerprint::empty();
        ArtifactFingerprint::of_artifact(&content, definitions)
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
    /// Hash public ABI facts through their codec-owned records and reference table.
    /// Scope validation does not grant ABI verification or reload compatibility.
    pub fn of_public_item(
        item: &PublicItem,
        definitions: &DefinitionTable,
    ) -> Result<Self, DefinitionMappingError> {
        let cancel = CancellationToken::default();
        let mut references = HashSet::new();
        item.visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            &cancel,
        )?;
        let encoding = definitions.encode(references)?;
        let record = PublicItemRecord::encode(item, &encoding, &cancel)?;
        Ok(Self::of_serialized(&(record, encoding.table)))
    }

    /// Hash an executable program's canonical referenced closure.
    pub fn of_program(
        program: &BytecodeProgram,
        definitions: &DefinitionTable,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self::of_serialized(&wire::encode_program(
            program,
            definitions,
            &CancellationToken::default(),
        )?))
    }

    /// Hash an executable module's canonical referenced closure.
    pub fn of_module(
        module: &BytecodeModule,
        definitions: &DefinitionTable,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self::of_serialized(&wire::encode_module(
            module,
            definitions,
            &CancellationToken::default(),
        )?))
    }

    /// Hash an artifact envelope's canonical referenced closure, including its header.
    pub fn of_artifact(
        artifact: &KbcArtifact,
        definitions: &DefinitionTable,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self::of_serialized(&wire::encode_artifact(
            artifact,
            definitions,
            &CancellationToken::default(),
        )?))
    }

    pub fn empty() -> Self {
        Self(0)
    }

    /// FNV-1a/64 over domain-separated fixed-width little-endian bincode v1.
    /// This is a compatibility fingerprint, not authentication or a signature.
    pub fn of_serialized(value: &impl Serialize) -> Self {
        struct Sink(u64);

        impl Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                for byte in bytes {
                    self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
                }
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Sink(Self::of_str("kagari-canonical-v2").0);
        DefaultOptions::new()
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
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub sections: ArtifactSectionBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub source_files: SourceFileTable,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
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
                ArtifactSectionId::TraitContracts,
                count(|m| m.trait_contracts.len()),
            ),
            (
                ArtifactSectionId::ModuleSlots,
                count(|m| m.module_slots.len()),
            ),
            (
                ArtifactSectionId::StructLayouts,
                count(|m| m.structures.len()),
            ),
            (
                ArtifactSectionId::EnumLayouts,
                count(|m| m.enumerations.len()),
            ),
            (ArtifactSectionId::Paths, count(|m| m.paths.len())),
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

    fn with_metadata(
        program: &BytecodeProgram,
        debug: Option<&DebugMetadata>,
        signatures: Option<&ArtifactSignatures>,
        portable_mir: Option<&PortableMir>,
    ) -> Self {
        let mut tables = Self::from_program(program);
        if let Some(debug) = debug {
            push_section(
                &mut tables.sections,
                ArtifactSectionId::Debug,
                debug.source_files.len() + debug.debug_names.len(),
            );
            tables.source_files = debug.source_files.clone();
            tables.debug_names = debug.debug_names.clone();
        }
        if let Some(signatures) = signatures {
            push_section(
                &mut tables.sections,
                ArtifactSectionId::Signatures,
                signatures.signatures.len(),
            );
        }
        if let Some(portable_mir) = portable_mir {
            tables.sections.push(ArtifactSection {
                id: ArtifactSectionId::PortableMir,
                record_count: 1,
                fingerprint: ArtifactFingerprint::of_serialized(&(
                    ArtifactSectionId::PortableMir,
                    portable_mir,
                )),
            });
        }
        tables
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
    TraitContracts,
    ModuleSlots,
    StructLayouts,
    EnumLayouts,
    Paths,
    StringTable,
    SourceFiles,
    DebugNames,
    Verification,
    Debug,
    Signatures,
    PortableMir,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSection {
    pub id: ArtifactSectionId,
    pub record_count: usize,
    pub fingerprint: ArtifactFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub loader: LoaderValidationMetadata,
}

impl VerificationMetadata {
    fn build(
        program: &BytecodeProgram,
        options: &ArtifactBuildOptions,
        definitions: &DefinitionTable,
    ) -> Result<Self, DefinitionMappingError> {
        let module = &program.modules[program.root.index()];
        let typed_path_fingerprints = module
            .paths
            .iter()
            .map(|path| PathDescriptorFingerprint {
                path: path.id,
                fingerprint: ArtifactFingerprint::of_serialized(&(
                    path.contract_fingerprint,
                    path.root_ty,
                    path.result_ty,
                    path.read_only,
                )),
            })
            .collect::<Vec<_>>();
        let public_abi_fingerprints = module
            .public_items
            .iter()
            .map(|item| {
                Ok(PublicAbiFingerprint {
                    name: item.fingerprint_name(definitions)?,
                    fingerprint: ArtifactFingerprint::of_public_item(item, definitions)?,
                })
            })
            .collect::<Result<Vec<_>, DefinitionMappingError>>()?;

        Ok(Self {
            bytecode_verified: true,
            function_layouts: module
                .functions
                .iter()
                .map(|function| FunctionLayoutMetadata {
                    function: function.id,
                    semantic: function.metadata.semantic.clone(),
                    params: function.metadata.params.clone(),
                    return_type: function.metadata.return_type,
                    slots: function.metadata.slots.clone(),
                    roots: function.metadata.roots.clone(),
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
            dependency_fingerprints: program.dependency_fingerprints(definitions)?,

            loader: LoaderValidationMetadata {
                module_identity: module.identity.clone(),
                runtime_abi_version: options.runtime_abi_version.clone(),
                runtime_helper_abi_version: options.runtime_helper_abi_version.clone(),
                dependency_fingerprints: program.dependency_fingerprints(definitions)?,
                typed_path_fingerprints,
                public_abi_fingerprints,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionLayoutMetadata {
    pub semantic: BytecodeSemanticSlots,
    pub function: FunctionRef,
    pub params: Vec<ValueType>,
    pub return_type: ValueType,
    pub slots: Vec<ValueType>,
    pub roots: RootSlotLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionEffectMetadata {
    pub function: FunctionRef,
    pub effects: EffectSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlFlowTargetMetadata {
    pub function: FunctionRef,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub targets: Vec<JumpTarget>,
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
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub typed_path_fingerprints: PathFingerprintBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub public_abi_fingerprints: PublicAbiFingerprintBuffer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactBuildOptions {
    pub module_epoch: Option<ModuleEpoch>,
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,

    pub debug: Option<DebugMetadata>,
    pub signatures: Option<ArtifactSignatures>,
    pub portable_mir: Option<PortableMir>,
}

impl Default for ArtifactBuildOptions {
    fn default() -> Self {
        Self {
            module_epoch: None,
            runtime_abi_version: KAGARI_RUNTIME_ABI_VERSION.to_owned(),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.to_owned(),

            debug: None,
            signatures: None,
            portable_mir: None,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugMetadata {
    pub stripped: bool,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub source_files: SourceFileTable,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub debug_names: DebugNameTable,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
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
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    pub signatures: Vec<ArtifactSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSignature {
    pub key_id: String,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArtifactValidationError {
    #[error("artifact identity conversion failed: {0}")]
    Identity(#[from] DefinitionMappingError),
    #[error("invalid KBC magic bytes {0:?}")]
    InvalidMagic([u8; 4]),
    #[error("artifact format version mismatch: expected {expected}, found {found}")]
    FormatVersionMismatch { expected: u16, found: u16 },
    #[error("artifact language version mismatch: expected `{expected}`, found `{found}`")]
    LanguageVersionMismatch { expected: String, found: String },
    #[error("artifact runtime ABI mismatch: expected `{expected}`, found `{found}`")]
    RuntimeAbiMismatch { expected: String, found: String },
    #[error("artifact runtime helper ABI mismatch: expected `{expected}`, found `{found}`")]
    RuntimeHelperAbiMismatch { expected: String, found: String },
    #[error("artifact module identity mismatch: expected `{expected}`, found `{found}`")]
    ModuleIdentityMismatch {
        expected: Box<ModuleIdentity>,
        found: Box<ModuleIdentity>,
    },
    #[error("artifact content hash mismatch")]
    ContentHashMismatch,
    #[error("artifact bytecode was not verified")]
    UnverifiedBytecode,
    #[error("artifact verification metadata differs from its program")]
    VerificationMetadataMismatch,
    #[error("artifact tables differ from their program")]
    TableMismatch,
    #[error("artifact resource limit exceeded: {0}")]
    ResourceLimit(&'static str),
    #[error("artifact dependency fingerprints mismatch")]
    DependencyFingerprintMismatch,

    #[error("artifact typed path fingerprints mismatch")]
    PathFingerprintMismatch,
    #[error("artifact public ABI fingerprints mismatch")]
    PublicAbiFingerprintMismatch,
    #[error("artifact bytecode verification failed: {0}")]
    Bytecode(BytecodeVerificationError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("artifact codec error: {message}")]
pub struct ArtifactCodecError {
    message: String,
}

impl ArtifactCodecError {
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<Box<ErrorKind>> for ArtifactCodecError {
    fn from(error: Box<ErrorKind>) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl ArtifactValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Identity(_) => "KG_ARTIFACT_IDENTITY",
            Self::InvalidMagic(_) => "KG_ARTIFACT_INVALID_MAGIC",
            Self::FormatVersionMismatch { .. } => "KG_ARTIFACT_FORMAT_VERSION_MISMATCH",
            Self::LanguageVersionMismatch { .. } => "KG_ARTIFACT_LANGUAGE_VERSION_MISMATCH",
            Self::RuntimeAbiMismatch { .. } => "KG_ARTIFACT_RUNTIME_ABI_MISMATCH",
            Self::RuntimeHelperAbiMismatch { .. } => "KG_ARTIFACT_RUNTIME_HELPER_ABI_MISMATCH",
            Self::ModuleIdentityMismatch { .. } => "KG_ARTIFACT_MODULE_IDENTITY_MISMATCH",
            Self::ContentHashMismatch => "KG_ARTIFACT_CONTENT_HASH_MISMATCH",
            Self::UnverifiedBytecode => "KG_ARTIFACT_UNVERIFIED_BYTECODE",
            Self::VerificationMetadataMismatch => "KG_ARTIFACT_VERIFICATION_METADATA_MISMATCH",
            Self::TableMismatch => "KG_ARTIFACT_TABLE_MISMATCH",
            Self::ResourceLimit(_) => "KG_ARTIFACT_RESOURCE_LIMIT",
            Self::DependencyFingerprintMismatch => "KG_ARTIFACT_DEPENDENCY_FINGERPRINT_MISMATCH",

            Self::PathFingerprintMismatch => "KG_ARTIFACT_PATH_FINGERPRINT_MISMATCH",
            Self::PublicAbiFingerprintMismatch => "KG_ARTIFACT_PUBLIC_ABI_FINGERPRINT_MISMATCH",
            Self::Bytecode(error) => error.code(),
        }
    }
}

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
mod canonical_tests;

mod mapping;

#[cfg(test)]
mod identity_tests;
