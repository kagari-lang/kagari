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
pub const KBC_ARTIFACT_FORMAT_VERSION: u16 = 30;
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_ARTIFACT_MODULES: usize = crate::decode_limits::MAX_MODULES;
pub const MAX_ARTIFACT_FUNCTIONS: usize = crate::decode_limits::MAX_FUNCTIONS;
pub const MAX_ARTIFACT_INSTRUCTIONS: usize = crate::decode_limits::MAX_INSTRUCTIONS;
pub const MAX_ARTIFACT_TABLE_RECORDS: usize = crate::decode_limits::MAX_TABLE_RECORDS;
pub const MAX_ARTIFACT_NESTED_RECORDS: usize = crate::decode_limits::MAX_NESTED_RECORDS;

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
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
) -> Result<(), ArtifactValidationError> {
    if let Some(reason) = program_count_limit(program) {
        return Err(ArtifactValidationError::ResourceLimit(reason));
    }
    if exceeds_encoded_size(program) {
        return Err(ArtifactValidationError::ResourceLimit(
            "artifact encoded size limit exceeded",
        ));
    }
    Ok(())
}
pub const KAGARI_LANGUAGE_VERSION: &str = "kagari-language-v1";
pub const KAGARI_COMPILER_FINGERPRINT: &str = concat!("kagari-ir/", env!("CARGO_PKG_VERSION"));
pub const KAGARI_RUNTIME_ABI_VERSION: &str = "kagari-runtime-abi-v30";
pub const KAGARI_RUNTIME_HELPER_ABI_VERSION: &str = "kagari-runtime-helper-abi-v5";

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
    pub fn from_program(
        program: BytecodeProgram,
        options: ArtifactBuildOptions,
    ) -> Result<Self, ArtifactValidationError> {
        if let Some(reason) = program_count_limit(&program) {
            return Err(ArtifactValidationError::ResourceLimit(reason));
        }
        if let Some(reason) =
            metadata_count_limit(options.debug.as_ref(), options.signatures.as_ref())
        {
            return Err(ArtifactValidationError::ResourceLimit(reason));
        }
        if exceeds_encoded_size(&(&program, &options)) {
            return Err(ArtifactValidationError::ResourceLimit(
                "artifact encoded size limit exceeded",
            ));
        }
        let verification = VerificationMetadata::from_program(&program, &options)?;
        let module = &program.modules[program.root.index()];
        let debug = options.debug;
        let signatures = options.signatures;
        let tables = ArtifactTables::with_metadata(&program, debug.as_ref(), signatures.as_ref());
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
        if let Some(reason) = artifact_count_limit(&artifact) {
            return Err(ArtifactValidationError::ResourceLimit(reason));
        }
        artifact.header.content_hash = artifact.compute_content_hash();
        Ok(artifact)
    }

    pub fn validate_for_loader(
        &self,
        requirements: &ArtifactCompatibility,
    ) -> Result<(), ArtifactValidationError> {
        if let Some(reason) = artifact_count_limit(self) {
            return Err(ArtifactValidationError::ResourceLimit(reason));
        }
        self.validate_header(requirements)?;
        verify_program(&self.program).map_err(ArtifactValidationError::Bytecode)?;
        if self.header.content_hash != self.compute_content_hash() {
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
        );
        if self.verification != derived {
            return Err(ArtifactValidationError::VerificationMetadataMismatch);
        }
        if self.tables
            != ArtifactTables::with_metadata(
                &self.program,
                self.debug.as_ref(),
                self.signatures.as_ref(),
            )
        {
            return Err(ArtifactValidationError::TableMismatch);
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, ArtifactCodecError> {
        if let Some(reason) = artifact_count_limit(self) {
            return Err(ArtifactCodecError {
                message: reason.into(),
            });
        }
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
        let artifact: Self = codec()
            .deserialize(bytes)
            .map_err(ArtifactCodecError::from)?;
        if let Some(reason) = artifact_count_limit(&artifact) {
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
        let mut types = interface.types.clone();
        for ty in &mut types {
            ty.clear_documentation();
        }
        types.sort_by(|a, b| a.id.cmp(&b.id));
        Self::of_serialized(&("kagari-required-host-interface-v3", types, functions))
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
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub sections: ArtifactSectionBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub source_files: SourceFileTable,
    #[serde(deserialize_with = "crate::decode_limits::table")]
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
            (
                ArtifactSectionId::EnumLayouts,
                count(|m| m.enumerations.len()),
            ),
            (ArtifactSectionId::Paths, count(|m| m.paths.len())),
            (
                ArtifactSectionId::HostDependencies,
                count(|m| m.host_interface.functions.len() + m.host_interface.types.len()),
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
    fn with_metadata(
        program: &BytecodeProgram,
        debug: Option<&DebugMetadata>,
        signatures: Option<&ArtifactSignatures>,
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
        tables
    }
}

fn within_table_limit(lengths: impl IntoIterator<Item = usize>) -> bool {
    lengths
        .into_iter()
        .all(|length| length <= MAX_ARTIFACT_TABLE_RECORDS)
}

fn module_nested_count_limit(module: &BytecodeModule, total: &mut usize) -> bool {
    let mut add = |length: usize| {
        *total = total.saturating_add(length);
        length <= MAX_ARTIFACT_NESTED_RECORDS && *total <= MAX_ARTIFACT_TABLE_RECORDS
    };
    for function in &module.functions {
        if function
            .identity
            .as_ref()
            .is_some_and(|identity| !add(identity.arguments.len()))
        {
            return false;
        }
    }
    for record in &module.function_table {
        if record
            .identity
            .as_ref()
            .is_some_and(|identity| !add(identity.arguments.len()))
        {
            return false;
        }
    }
    for table in &module.interface_tables {
        if !add(table.methods.len()) {
            return false;
        }
    }
    for layout in &module.structures {
        if !add(layout.arguments.len()) || !add(layout.fields.len()) {
            return false;
        }
    }
    for layout in &module.enumerations {
        if !add(layout.arguments.len()) || !add(layout.variants.len()) {
            return false;
        }
        for variant in &layout.variants {
            if !add(variant.payload.len()) {
                return false;
            }
        }
    }
    for ty in &module.host_interface.types {
        if !add(ty.fields.len()) || !add(ty.methods.len()) {
            return false;
        }
        for method in &ty.methods {
            if !add(method.params.len()) {
                return false;
            }
        }
    }
    for function in &module.host_interface.functions {
        if !add(function.params.len()) {
            return false;
        }
    }
    for path in &module.host_interface.paths {
        if !add(path.segments.len()) {
            return false;
        }
    }
    for item in &module.public_items {
        use crate::module::PublicAbiItem;
        let valid = match item {
            PublicAbiItem::Function(item) => {
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.params.len())
            }
            PublicAbiItem::Const(_) => true,
            PublicAbiItem::Type(item) => {
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.fields.len())
                    && add(item.variants.len())
                    && item
                        .variants
                        .iter()
                        .all(|variant| add(variant.payload.len()))
            }
            PublicAbiItem::Trait(item) => {
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.methods.len())
                    && item.methods.iter().all(|method| {
                        add(method.generic_params.len())
                            && add(method.bounds.len())
                            && add_abi_bounds(&method.bounds, &mut add)
                            && add(method.params.len())
                    })
            }
            PublicAbiItem::InterfaceTable(item) => {
                add(item.generic_params.len())
                    && add(item.bounds.len())
                    && add_abi_bounds(&item.bounds, &mut add)
                    && add(item.methods.len())
                    && item.methods.iter().all(|method| {
                        add(method.generic_params.len())
                            && add(method.bounds.len())
                            && add_abi_bounds(&method.bounds, &mut add)
                            && add(method.params.len())
                    })
            }
        };
        if !valid {
            return false;
        }
    }
    true
}

fn add_abi_bounds(
    bounds: &[crate::module::abi::GenericBoundAbi],
    add: &mut impl FnMut(usize) -> bool,
) -> bool {
    bounds.iter().all(|bound| {
        add(bound.constraints.len())
            && bound.constraints.iter().all(|constraint| match constraint {
                crate::module::abi::ConstraintAbi::Trait(ty) => add(ty.arguments.len()),
                _ => true,
            })
    })
}

fn generic_identity_limit(
    params: &[crate::module::abi::GenericParameterAbi],
    bounds: &[crate::module::abi::GenericBoundAbi],
) -> bool {
    params.iter().all(|param| param.owner.within_path_limit())
        && bounds.iter().all(|bound| {
            bound.owner.within_path_limit()
                && bound.constraints.iter().all(|constraint| match constraint {
                    crate::module::abi::ConstraintAbi::Standard(_) => true,
                    crate::module::abi::ConstraintAbi::Trait(ty) => {
                        ty.declaration.within_path_limit()
                            && ty.arguments.iter().all(|arg| arg.within_wire_limits())
                    }
                })
        })
}

fn function_abi_identity_limit(function: &crate::module::FunctionAbi) -> bool {
    generic_identity_limit(&function.generic_params, &function.bounds)
}

fn module_abi_type_limit(module: &BytecodeModule) -> bool {
    use crate::module::PublicAbiItem;
    let valid = |ty: &crate::module::abi::AbiType| ty.within_wire_limits();
    module.interface_tables.iter().all(|table| {
        table.declaration.within_path_limit()
            && table
                .methods
                .iter()
                .all(|slot| slot.method.within_path_limit())
    }) && module.functions.iter().all(|function| {
        function.identity.as_ref().is_none_or(|identity| {
            identity.declaration.within_path_limit() && identity.arguments.iter().all(&valid)
        })
    }) && module.function_table.iter().all(|record| {
        record.identity.as_ref().is_none_or(|identity| {
            identity.declaration.within_path_limit() && identity.arguments.iter().all(&valid)
        })
    }) && module.structures.iter().all(|layout| {
        layout.arguments.iter().all(&valid) && layout.fields.iter().all(|field| valid(&field.ty))
    }) && module.enumerations.iter().all(|layout| {
        layout.arguments.iter().all(&valid)
            && layout
                .variants
                .iter()
                .all(|variant| variant.payload.iter().all(&valid))
    }) && module.public_items.iter().all(|item| match item {
        PublicAbiItem::Function(item) => {
            function_abi_identity_limit(item)
                && item.params.iter().all(|param| valid(&param.ty))
                && valid(&item.return_type)
        }
        PublicAbiItem::Const(item) => valid(&item.ty),
        PublicAbiItem::Type(item) => {
            generic_identity_limit(&item.generic_params, &item.bounds)
                && item.fields.iter().all(|field| valid(&field.ty))
                && item
                    .variants
                    .iter()
                    .all(|variant| variant.payload.iter().all(&valid))
        }
        PublicAbiItem::Trait(item) => {
            generic_identity_limit(&item.generic_params, &item.bounds)
                && item.methods.iter().all(|method| {
                    function_abi_identity_limit(method)
                        && method.params.iter().all(|param| valid(&param.ty))
                        && valid(&method.return_type)
                })
        }
        PublicAbiItem::InterfaceTable(item) => {
            item.declaration.within_path_limit()
                && generic_identity_limit(&item.generic_params, &item.bounds)
                && valid(&item.trait_type)
                && valid(&item.for_type)
                && item.methods.iter().all(|method| {
                    function_abi_identity_limit(method)
                        && method.params.iter().all(|param| valid(&param.ty))
                        && valid(&method.return_type)
                })
        }
    })
}

fn host_identity_limit(interface: &kagari_common::host_interface::HostInterface) -> bool {
    let valid = |id: &kagari_common::identity::DefinitionId| id.within_path_limit();
    let value = |ty: &kagari_common::host_interface::HostValueType| {
        ty.nominal_references().into_iter().all(valid)
    };
    interface.types.iter().all(|ty| {
        valid(&ty.id)
            && ty
                .fields
                .iter()
                .all(|field| valid(&field.id) && value(&field.ty))
            && ty.methods.iter().all(|method| {
                valid(&method.id)
                    && method.params.iter().all(|param| value(&param.ty))
                    && value(&method.return_type)
            })
    }) && interface.functions.iter().all(|function| {
        valid(&function.id)
            && function.params.iter().all(|param| value(&param.ty))
            && value(&function.return_type)
    }) && interface.paths.iter().all(|path| {
        valid(&path.root)
            && path.segments.iter().all(|segment| match segment {
                kagari_common::host_interface::HostPathSegmentDeclaration::Field(id) => valid(id),
                kagari_common::host_interface::HostPathSegmentDeclaration::Index(index) => {
                    value(&index.collection) && value(&index.index) && value(&index.result)
                }
                kagari_common::host_interface::HostPathSegmentDeclaration::Virtual(
                    virtual_step,
                ) => value(&virtual_step.result),
            })
    })
}

fn program_count_limit(program: &BytecodeProgram) -> Option<&'static str> {
    if program.modules.len() > MAX_ARTIFACT_MODULES {
        return Some("too many modules");
    }
    let mut functions = 0usize;
    let mut instructions = 0usize;
    let mut operand_records = 0usize;
    let mut module_records = 0usize;
    let mut nested_records = 0usize;
    for module in &program.modules {
        if !module.identity.within_path_limit()
            || !host_identity_limit(&module.host_interface)
            || module.structures.iter().any(|layout| {
                !layout.declaration.within_path_limit()
                    || layout
                        .fields
                        .iter()
                        .any(|field| !field.declaration.within_path_limit())
            })
            || module.enumerations.iter().any(|layout| {
                !layout.declaration.within_path_limit()
                    || layout
                        .variants
                        .iter()
                        .any(|variant| !variant.declaration.within_path_limit())
            })
        {
            return Some("identity path segment limit exceeded");
        }
        if !module_nested_count_limit(module, &mut nested_records) {
            return Some("nested module record limit exceeded");
        }
        if !module_abi_type_limit(module) {
            return Some("ABI type resource limit exceeded");
        }
        functions = functions.saturating_add(module.functions.len());
        if functions > MAX_ARTIFACT_FUNCTIONS {
            return Some("too many functions");
        }
        module_records = module_records.saturating_add(
            [
                module.dependencies.len(),
                module.host_interface.types.len(),
                module.host_interface.functions.len(),
                module.host_interface.paths.len(),
                module.module_slots.len(),
                module.constants.len(),
                module.types.len(),
                module.structures.len(),
                module.enumerations.len(),
                module.interface_tables.len(),
                module.paths.len(),
                module.function_table.len(),
                module.public_items.len(),
            ]
            .into_iter()
            .fold(0usize, usize::saturating_add),
        );
        if module_records > MAX_ARTIFACT_TABLE_RECORDS {
            return Some("module table record limit exceeded");
        }
        if module
            .function_table
            .iter()
            .any(|record| record.params.len() > MAX_ARTIFACT_TABLE_RECORDS)
        {
            return Some("function table parameter record limit exceeded");
        }
        for function in &module.functions {
            instructions = instructions.saturating_add(function.instructions.len());
            if instructions > MAX_ARTIFACT_INSTRUCTIONS {
                return Some("too many instructions");
            }
            for instruction in &function.instructions {
                let count = instruction.operand_vector_len();
                if count > MAX_ARTIFACT_NESTED_RECORDS {
                    return Some("instruction operand record limit exceeded");
                }
                operand_records = operand_records.saturating_add(count);
                if operand_records > MAX_ARTIFACT_TABLE_RECORDS {
                    return Some("instruction operand aggregate limit exceeded");
                }
            }
            let metadata = &function.metadata;
            let debug = &metadata.debug;
            if !within_table_limit([
                metadata.params.len(),
                metadata.locals.len(),
                metadata.registers.len(),
                metadata.control_flow_targets.len(),
                debug.source_spans.len(),
                debug.line_table.len(),
                debug.safe_debug_points.len(),
                debug.local_live_ranges.len(),
                debug.captured_bindings.len(),
                debug.frame_layout.params.len(),
                debug.frame_layout.locals.len(),
                debug.frame_layout.registers.len(),
            ]) {
                return Some("function metadata record limit exceeded");
            }
        }
    }
    None
}

fn metadata_count_limit(
    debug: Option<&DebugMetadata>,
    signatures: Option<&ArtifactSignatures>,
) -> Option<&'static str> {
    if let Some(debug) = debug
        && (!within_table_limit([
            debug.source_files.len(),
            debug.debug_names.len(),
            debug.functions.len(),
        ]) || debug.functions.iter().any(|function| {
            !within_table_limit([
                function.source_spans.len(),
                function.line_table.len(),
                function.safe_debug_points.len(),
                function.local_live_ranges.len(),
                function.captured_bindings.len(),
                function.frame_layout.params.len(),
                function.frame_layout.locals.len(),
                function.frame_layout.registers.len(),
            ])
        }))
    {
        return Some("debug record limit exceeded");
    }
    if signatures.is_some_and(|signatures| signatures.signatures.len() > MAX_ARTIFACT_TABLE_RECORDS)
    {
        return Some("signature record limit exceeded");
    }
    None
}

fn artifact_count_limit(artifact: &KbcArtifact) -> Option<&'static str> {
    if !artifact.header.module_identity.within_path_limit()
        || !artifact
            .verification
            .loader
            .module_identity
            .within_path_limit()
        || artifact
            .verification
            .dependency_fingerprints
            .iter()
            .chain(&artifact.verification.loader.dependency_fingerprints)
            .any(|dependency| !dependency.module_id.within_path_limit())
    {
        return Some("identity path segment limit exceeded");
    }
    if let Some(reason) = program_count_limit(&artifact.program) {
        return Some(reason);
    }
    if let Some(reason) =
        metadata_count_limit(artifact.debug.as_ref(), artifact.signatures.as_ref())
    {
        return Some(reason);
    }
    let tables = &artifact.tables;
    let verification = &artifact.verification;
    if !within_table_limit([
        tables.sections.len(),
        tables.source_files.len(),
        tables.debug_names.len(),
        verification.function_layouts.len(),
        verification.function_effects.len(),
        verification.control_flow_targets.len(),
        verification.typed_path_fingerprints.len(),
        verification.public_abi_fingerprints.len(),
        verification.dependency_fingerprints.len(),
        verification.security_profile_requirements.len(),
        verification.loader.dependency_fingerprints.len(),
        verification.loader.typed_path_fingerprints.len(),
        verification.loader.public_abi_fingerprints.len(),
    ]) || tables
        .sections
        .iter()
        .any(|section| section.record_count > MAX_ARTIFACT_TABLE_RECORDS)
        || verification
            .control_flow_targets
            .iter()
            .any(|item| item.targets.len() > MAX_ARTIFACT_TABLE_RECORDS)
        || verification.function_layouts.iter().any(|item| {
            !within_table_limit([item.params.len(), item.locals.len(), item.registers.len()])
        })
    {
        return Some("artifact metadata record limit exceeded");
    }
    if exceeds_encoded_size(artifact) {
        return Some("artifact encoded size limit exceeded");
    }
    None
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
    EnumLayouts,
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
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub function_layouts: FunctionLayoutBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub function_effects: FunctionEffectBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub control_flow_targets: ControlFlowTargetMetadataBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub typed_path_fingerprints: PathFingerprintBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub public_abi_fingerprints: PublicAbiFingerprintBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    pub host_interface_fingerprint: ArtifactFingerprint,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub security_profile_requirements: Vec<String>,
    pub loader: LoaderValidationMetadata,
}

impl VerificationMetadata {
    pub fn from_program(
        program: &BytecodeProgram,
        options: &ArtifactBuildOptions,
    ) -> Result<Self, ArtifactValidationError> {
        verify_program(program).map_err(ArtifactValidationError::Bytecode)?;
        Ok(Self::build(program, options))
    }
    fn build(program: &BytecodeProgram, options: &ArtifactBuildOptions) -> Self {
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
            .map(|item| PublicAbiFingerprint {
                name: item.fingerprint_name(),
                fingerprint: ArtifactFingerprint::of_serialized(item),
            })
            .collect::<Vec<_>>();

        Self {
            bytecode_verified: true,
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
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub params: Vec<ValueType>,
    pub return_type: ValueType,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub locals: Vec<ValueType>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
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
    #[serde(deserialize_with = "crate::decode_limits::table")]
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
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub dependency_fingerprints: DependencyFingerprintBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub typed_path_fingerprints: PathFingerprintBuffer,
    #[serde(deserialize_with = "crate::decode_limits::table")]
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
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub source_files: SourceFileTable,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub debug_names: DebugNameTable,
    #[serde(deserialize_with = "crate::decode_limits::table")]
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
    #[serde(deserialize_with = "crate::decode_limits::table")]
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
    TableMismatch,
    ResourceLimit(&'static str),
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
            Self::TableMismatch => "KG_ARTIFACT_TABLE_MISMATCH",
            Self::ResourceLimit(_) => "KG_ARTIFACT_RESOURCE_LIMIT",
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
            Self::TableMismatch => write!(f, "artifact tables differ from their program"),
            Self::ResourceLimit(reason) => write!(f, "artifact resource limit exceeded: {reason}"),
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
    fn memory_artifacts_reject_oversized_strings_before_fingerprinting() {
        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        {
            let mut program = valid.clone();
            program.modules[0].source_name = "x".repeat(MAX_ARTIFACT_BYTES as usize);
            assert!(matches!(
                KbcArtifact::from_program(program, Default::default()),
                Err(ArtifactValidationError::ResourceLimit(
                    "artifact encoded size limit exceeded"
                ))
            ));
        }
        let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
        artifact.header.compiler_fingerprint = "x".repeat(MAX_ARTIFACT_BYTES as usize);
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "artifact encoded size limit exceeded"
            ))
        ));
        assert!(
            artifact
                .to_bytes()
                .unwrap_err()
                .message()
                .contains("artifact encoded size limit exceeded")
        );
    }

    #[test]
    fn typed_path_operands_preflight_before_decoding_registers() {
        let instruction = crate::bytecode::BytecodeInstruction::ReadPath {
            dst: crate::bytecode::Register::new(0),
            root_or_view: crate::bytecode::Register::new(1),
            path: crate::bytecode::PathId::new(0),
            dynamic_args: vec![crate::bytecode::Register::new(2); MAX_ARTIFACT_NESTED_RECORDS + 1],
        };
        let bytes = codec().serialize(&instruction).unwrap();
        let error = codec()
            .deserialize::<crate::bytecode::BytecodeInstruction>(&bytes)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("instruction operand count limit exceeded")
        );
    }

    #[test]
    fn instruction_operand_vectors_are_bounded_before_verification() {
        use crate::bytecode::{BytecodeFunction, BytecodeInstruction, Register};

        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        let make_tuple = |count| BytecodeInstruction::MakeTuple {
            dst: Register::new(0),
            elements: vec![Register::new(0); count],
        };
        let mut oversized = valid.clone();
        let mut function = BytecodeFunction::default();
        function
            .instructions
            .push(make_tuple(MAX_ARTIFACT_NESTED_RECORDS + 1));
        oversized.modules[0].functions.push(function);
        assert!(matches!(
            KbcArtifact::from_program(oversized.clone(), Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "instruction operand record limit exceeded"
            ))
        ));
        let mut artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
        artifact.program = oversized;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "instruction operand record limit exceeded"
            ))
        ));
        assert!(artifact.to_bytes().is_err());
        let crafted = codec().serialize(&artifact).unwrap();
        assert!(
            KbcArtifact::from_bytes(&crafted)
                .unwrap_err()
                .message()
                .contains("instruction operand count limit exceeded")
        );

        let mut aggregate = valid;
        let function = BytecodeFunction {
            instructions: vec![
                make_tuple(MAX_ARTIFACT_NESTED_RECORDS);
                MAX_ARTIFACT_TABLE_RECORDS / MAX_ARTIFACT_NESTED_RECORDS + 1
            ],
            ..Default::default()
        };
        aggregate.modules[0].functions.push(function);
        assert!(matches!(
            KbcArtifact::from_program(aggregate, Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "instruction operand aggregate limit exceeded"
            ))
        ));
    }

    #[test]
    fn oversized_memory_identity_paths_reject_before_fingerprinting() {
        use crate::module::abi::AbiType;
        use kagari_common::host_interface::{
            HostInterface, HostTypeDeclaration, host_type_identity,
        };
        use kagari_common::identity::MAX_IDENTITY_PATH_SEGMENTS;

        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        for case in 0..3 {
            let mut program = valid.clone();
            let reason = if case == 2 {
                "ABI type resource limit exceeded"
            } else {
                "identity path segment limit exceeded"
            };
            match case {
                0 => {
                    program.modules[0].identity.path =
                        vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
                }
                1 => {
                    let mut host = HostTypeDeclaration::new("demo.Player");
                    host.id.module.path = vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
                    program.modules[0].host_interface = HostInterface {
                        types: vec![host],
                        ..Default::default()
                    };
                }
                _ => {
                    let mut id = host_type_identity("demo.Player");
                    id.module.path = vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
                    program.modules[0]
                        .public_items
                        .push(crate::module::PublicAbiItem::Const(
                            crate::module::ConstAbi {
                                name: "bad".into(),
                                ty: AbiType::Host(id),
                                value: "0".into(),
                            },
                        ));
                }
            }
            assert!(matches!(
                KbcArtifact::from_program(program.clone(), Default::default()),
                Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
            ));
            let mut artifact =
                KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
            artifact.program = program;
            assert!(matches!(
                artifact.validate_for_loader(&Default::default()),
                Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
            ));
            assert!(artifact.to_bytes().is_err());
        }
        let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
        artifact.header.module_identity.path = vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "identity path segment limit exceeded"
            ))
        ));
        assert!(artifact.to_bytes().is_err());
    }

    #[test]
    fn nested_function_layout_tables_are_bounded_on_memory_and_wire_routes() {
        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        let mut function_table = valid.clone();
        function_table.modules[0]
            .function_table
            .push(crate::bytecode::FunctionRecord {
                id: FunctionRef::new(0),
                identity: None,
                name: "oversized".into(),
                params: vec![ValueType::Unit; MAX_ARTIFACT_TABLE_RECORDS + 1],
                return_type: ValueType::Unit,
                effects: Default::default(),
            });
        let mut debug_frame = valid.clone();
        let mut function = crate::bytecode::BytecodeFunction::default();
        function.metadata.debug.frame_layout.params =
            vec![ValueType::Unit; MAX_ARTIFACT_TABLE_RECORDS + 1];
        debug_frame.modules[0].functions.push(function);
        for (program, reason) in [
            (
                function_table,
                "function table parameter record limit exceeded",
            ),
            (debug_frame, "function metadata record limit exceeded"),
        ] {
            assert!(matches!(
                KbcArtifact::from_program(program.clone(), Default::default()),
                Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
            ));
            let mut artifact =
                KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
            artifact.program = program;
            assert!(matches!(
                artifact.validate_for_loader(&Default::default()),
                Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
            ));
            assert!(artifact.to_bytes().is_err());
            let crafted = codec().serialize(&artifact).unwrap();
            assert!(
                KbcArtifact::from_bytes(&crafted)
                    .unwrap_err()
                    .message()
                    .contains("artifact table count limit exceeded")
            );
        }
    }

    #[test]
    fn detached_debug_frame_layout_is_bounded_before_fingerprinting() {
        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        let mut debug = BytecodeDebugMetadata::default();
        debug.frame_layout.locals = vec![ValueType::Unit; MAX_ARTIFACT_TABLE_RECORDS + 1];
        let metadata = DebugMetadata {
            stripped: false,
            source_files: Vec::new(),
            debug_names: Vec::new(),
            functions: vec![debug],
        };
        assert!(matches!(
            KbcArtifact::from_program(
                valid.clone(),
                ArtifactBuildOptions {
                    debug: Some(metadata.clone()),
                    ..Default::default()
                }
            ),
            Err(ArtifactValidationError::ResourceLimit(
                "debug record limit exceeded"
            ))
        ));
        let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
        artifact.debug = Some(metadata);
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "debug record limit exceeded"
            ))
        ));
        assert!(artifact.to_bytes().is_err());
        let crafted = codec().serialize(&artifact).unwrap();
        assert!(
            KbcArtifact::from_bytes(&crafted)
                .unwrap_err()
                .message()
                .contains("artifact table count limit exceeded")
        );
    }

    #[test]
    fn decoder_rejects_forged_header_identity_path_length() {
        let artifact = KbcArtifact::from_program(
            BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            Default::default(),
        )
        .unwrap();
        let mut bytes = artifact.to_bytes().unwrap();
        let header = &artifact.header;
        let offset = codec()
            .serialized_size(&(
                &header.magic,
                &header.format_version,
                &header.language_version,
                &header.compiler_fingerprint,
                &header.runtime_abi_version,
                &header.runtime_helper_abi_version,
                &header.encoding,
                &header.module_identity.package,
            ))
            .unwrap() as usize;
        bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let error = KbcArtifact::from_bytes(&bytes).unwrap_err();
        assert!(
            error
                .message()
                .contains("module identity path segment count limit exceeded")
        );
    }

    #[test]
    fn debug_table_length_is_rejected_before_decoding_source_names() {
        let debug = DebugMetadata {
            stripped: false,
            source_files: Vec::new(),
            debug_names: Vec::new(),
            functions: Vec::new(),
        };
        let mut bytes = codec().serialize(&debug).unwrap();
        bytes[1..9].copy_from_slice(&u64::MAX.to_le_bytes());
        let error = codec().deserialize::<DebugMetadata>(&bytes).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("artifact table count limit exceeded")
        );
    }

    #[test]
    fn decoder_rejects_huge_module_count_before_reading_module_data() {
        let artifact = KbcArtifact::from_program(
            BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            Default::default(),
        )
        .unwrap();
        let mut bytes = artifact.to_bytes().unwrap();
        let offset = (codec().serialized_size(&artifact.header).unwrap()
            + codec().serialized_size(&artifact.program.root).unwrap())
            as usize;
        bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let error = KbcArtifact::from_bytes(&bytes).unwrap_err();
        assert!(error.message().contains("module count limit exceeded"));
    }

    #[test]
    fn deep_abi_types_are_rejected_before_artifact_fingerprinting() {
        use crate::module::abi::{AbiType, BuiltinType};
        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        let mut deep = AbiType::Builtin(BuiltinType::I32);
        for _ in 0..64 {
            deep = AbiType::Array(Box::new(deep));
        }
        let mut program = valid.clone();
        program.modules[0]
            .public_items
            .push(crate::module::PublicAbiItem::Const(
                crate::module::ConstAbi {
                    name: "deep".into(),
                    ty: deep,
                    value: "0".into(),
                },
            ));
        assert!(matches!(
            KbcArtifact::from_program(program.clone(), Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "ABI type resource limit exceeded"
            ))
        ));
        let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
        artifact.program = program;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "ABI type resource limit exceeded"
            ))
        ));
        assert!(artifact.to_bytes().is_err());
    }

    #[test]
    fn nested_layout_and_host_path_counts_are_bounded_on_all_artifact_routes() {
        use crate::module::abi::{AbiType, BuiltinType};
        use crate::module::{
            EnumLayout, EnumVariantLayout, FieldAbi, StructFieldLayout, StructLayout,
        };
        use kagari_common::host_interface::{HostPathDeclaration, PathAccess};
        use kagari_common::identity::{DefinitionId, DefinitionKind, DefinitionPathSegment};

        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        let id = |kind| DefinitionId {
            module: valid.modules[0].identity.clone(),
            path: vec![DefinitionPathSegment {
                kind,
                name: "item".into(),
                occurrence: 0,
            }],
        };
        let field_id = id(DefinitionKind::Field);
        let mut cases = Vec::new();

        let mut structure = valid.clone();
        structure.modules[0].structures.push(StructLayout {
            declaration: id(DefinitionKind::Struct),
            arguments: Vec::new(),
            fields: vec![
                StructFieldLayout {
                    declaration: field_id.clone(),
                    name: "field".into(),
                    ty: AbiType::Builtin(BuiltinType::I32),
                    mutable: false,
                };
                MAX_ARTIFACT_NESTED_RECORDS + 1
            ],
        });
        cases.push(structure);

        let mut enumeration = valid.clone();
        enumeration.modules[0].enumerations.push(EnumLayout {
            declaration: id(DefinitionKind::Enum),
            arguments: Vec::new(),
            variants: vec![EnumVariantLayout {
                declaration: id(DefinitionKind::Variant),
                payload: vec![AbiType::Builtin(BuiltinType::I32); MAX_ARTIFACT_NESTED_RECORDS + 1],
            }],
        });
        cases.push(enumeration);

        let mut host_path = valid.clone();
        host_path.modules[0]
            .host_interface
            .paths
            .push(HostPathDeclaration {
                root: id(DefinitionKind::Struct),
                segments: vec![
                    kagari_common::host_interface::HostPathSegmentDeclaration::Field(
                        field_id.clone()
                    );
                    MAX_ARTIFACT_NESTED_RECORDS + 1
                ],
                access: PathAccess::ReadOnly,
                schema_epoch: 0,
                capabilities: Default::default(),
            });
        cases.push(host_path);

        let mut public_abi = valid.clone();
        public_abi.modules[0]
            .public_items
            .push(crate::module::PublicAbiItem::Type(crate::module::TypeAbi {
                name: "item".into(),
                kind: crate::module::TypeAbiKind::Struct,
                generic_params: Vec::new(),
                bounds: Vec::new(),
                fields: vec![
                    FieldAbi {
                        name: "field".into(),
                        ty: AbiType::Builtin(BuiltinType::I32),
                        mutable: false,
                    };
                    MAX_ARTIFACT_NESTED_RECORDS + 1
                ],
                variants: Vec::new(),
            }));
        cases.push(public_abi);

        for (index, program) in cases.into_iter().enumerate() {
            assert!(matches!(
                KbcArtifact::from_program(program.clone(), Default::default()),
                Err(ArtifactValidationError::ResourceLimit(
                    "nested module record limit exceeded"
                ))
            ));
            let mut artifact =
                KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
            artifact.program = program;
            assert!(matches!(
                artifact.validate_for_loader(&Default::default()),
                Err(ArtifactValidationError::ResourceLimit(
                    "nested module record limit exceeded"
                ))
            ));
            assert!(artifact.to_bytes().is_err());
            let crafted = codec().serialize(&artifact).unwrap();
            let error = KbcArtifact::from_bytes(&crafted).unwrap_err();
            let expected = if index == 2 {
                "host member count limit exceeded"
            } else {
                "nested declaration count limit exceeded"
            };
            assert!(error.message().contains(expected), "{index}: {error}");
        }
    }

    #[test]
    fn module_count_limit_rejects_memory_and_encoded_artifacts_before_verification() {
        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        let mut excessive = valid.clone();
        excessive
            .modules
            .resize(MAX_ARTIFACT_MODULES + 1, BytecodeModule::default());
        assert!(matches!(
            KbcArtifact::from_program(excessive.clone(), Default::default()),
            Err(ArtifactValidationError::ResourceLimit("too many modules"))
        ));
        let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
        artifact.program = excessive;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit("too many modules"))
        ));
        assert!(
            artifact
                .to_bytes()
                .unwrap_err()
                .message()
                .contains("too many modules")
        );
        let crafted = codec().serialize(&artifact).unwrap();
        assert!(
            KbcArtifact::from_bytes(&crafted)
                .unwrap_err()
                .message()
                .contains("module count limit exceeded")
        );
    }

    #[test]
    fn declared_section_counts_are_bounded_independently_of_payload_size() {
        let mut artifact = KbcArtifact::from_program(
            BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            Default::default(),
        )
        .unwrap();
        artifact.tables.sections[0].record_count = MAX_ARTIFACT_TABLE_RECORDS + 1;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "artifact metadata record limit exceeded"
            ))
        ));
        assert!(artifact.to_bytes().is_err());
        let crafted = codec().serialize(&artifact).unwrap();
        assert!(KbcArtifact::from_bytes(&crafted).is_err());
    }

    #[test]
    fn recomputed_hash_cannot_hide_inconsistent_artifact_tables() {
        let artifact = KbcArtifact::from_program(
            BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            Default::default(),
        )
        .unwrap();
        assert!(artifact.validate_for_loader(&Default::default()).is_ok());
        for change in 0..5 {
            let mut changed = artifact.clone();
            match change {
                0 => changed.tables.sections[0].record_count += 1,
                1 => changed.tables.sections[0].fingerprint = ArtifactFingerprint(0),
                2 => changed.tables.sections.swap(0, 1),
                3 => {
                    changed.tables.sections.pop();
                }
                _ => changed.tables.source_files.push("forged.kgr".into()),
            }
            changed.header.content_hash = changed.compute_content_hash();
            assert!(
                matches!(
                    changed.validate_for_loader(&Default::default()),
                    Err(ArtifactValidationError::TableMismatch)
                ),
                "change {change}"
            );
        }
    }

    #[test]
    fn invalid_host_types_are_rejected_before_fingerprinting_memory_artifacts() {
        use kagari_common::host_interface::{HostFunctionDeclaration, HostValueType};
        let valid = BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        };
        let mut deep = HostValueType::I32;
        for _ in 0..64 {
            deep = HostValueType::Array(Box::new(deep));
        }
        for ty in [HostValueType::Set(Box::new(HostValueType::F32)), deep] {
            let mut program = valid.clone();
            program.modules[0]
                .host_interface
                .functions
                .push(HostFunctionDeclaration::new("host.bad", vec![], ty));
            assert!(matches!(
                VerificationMetadata::from_program(&program, &Default::default()),
                Err(ArtifactValidationError::Bytecode(_))
            ));
            assert!(matches!(
                KbcArtifact::from_program(program.clone(), Default::default()),
                Err(ArtifactValidationError::Bytecode(_))
            ));
            let mut artifact =
                KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
            artifact.program = program;
            assert!(matches!(
                artifact.validate_for_loader(&Default::default()),
                Err(ArtifactValidationError::Bytecode(_))
            ));
        }
    }

    #[test]
    fn bytecode_cannot_claim_a_different_identity_from_its_header() {
        let mut artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
            Default::default(),
        )
        .unwrap();
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
        )
        .unwrap();
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
        )
        .unwrap();
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
    fn artifact_preserves_portable_virtual_path_declarations() {
        use kagari_common::host_interface::{
            HostPathDeclaration, HostPathSegmentDeclaration, HostTypeDeclaration,
            HostTypeOwnership, HostValueType, HostVirtualSegmentDeclaration, PathAccess,
        };
        let mut root = HostTypeDeclaration::new("game.Player");
        root.ownership = HostTypeOwnership::HostRoot;
        root.path_access = PathAccess::ReadOnly;
        let path = HostPathDeclaration {
            root: root.id.clone(),
            segments: vec![HostPathSegmentDeclaration::Virtual(
                HostVirtualSegmentDeclaration {
                    name: "preview".into(),
                    result: HostValueType::I32,
                    access: PathAccess::ReadOnly,
                },
            )],
            access: PathAccess::ReadOnly,
            schema_epoch: 1,
            capabilities: Default::default(),
        };
        let mut module = BytecodeModule::default();
        module.host_interface.types.push(root);
        module.host_interface.paths.push(path.clone());
        let artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![module],
            },
            ArtifactBuildOptions::default(),
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded.program.modules[0].host_interface.paths, vec![path]);
    }

    #[test]
    fn required_host_fingerprint_is_derived_and_independent_of_docs_and_order() {
        use kagari_common::host_interface::{
            HostFunctionDeclaration, HostInterface, HostValueType, standard_log,
        };
        let interface = HostInterface {
            paths: vec![],
            types: Vec::new(),
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
        )
        .unwrap();
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
        )
        .unwrap();
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
        let original = KbcArtifact::from_program(program, Default::default()).unwrap();
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
