//! Artifact envelope fields and claimed verification summaries, without seals.
use crate::{
    artifact::{
        ArtifactFingerprint, ArtifactHeader, ArtifactSignatures, ArtifactTables,
        ControlFlowTargetMetadataBuffer, DebugMetadata, DependencyFingerprintBuffer,
        FunctionEffectBuffer, FunctionLayoutMetadata, KbcArtifact, LoaderValidationMetadata,
        PathFingerprintBuffer, PublicAbiFingerprintBuffer, VerificationMetadata,
        wire::module::BytecodeProgramRecord,
    },
    module::RootSlotLayout,
    native_input::PortableMir,
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_contract::{ids::FunctionRef, slots::wire::SemanticSlotsRecord};
use serde::{Deserialize, Serialize};

/// File-local KbcArtifact facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct KbcArtifactRecord {
    header: ArtifactHeader,
    program: BytecodeProgramRecord,
    tables: ArtifactTables,
    verification: VerificationMetadataRecord,
    debug: Option<DebugMetadata>,
    signatures: Option<ArtifactSignatures>,
    portable_mir: Option<PortableMir>,
}

impl KbcArtifactRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &KbcArtifact,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            header: value.header.clone(),
            program: BytecodeProgramRecord::encode(&value.program, encoding, cancel)?,
            tables: value.tables.clone(),
            verification: VerificationMetadataRecord::encode(
                &value.verification,
                encoding,
                cancel,
            )?,
            debug: value.debug.clone(),
            signatures: value.signatures.clone(),
            portable_mir: value.portable_mir.clone(),
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<KbcArtifact, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(KbcArtifact {
            header: self.header,
            program: self.program.decode(decoding, cancel)?,
            tables: self.tables,
            verification: self.verification.decode(decoding, cancel)?,
            debug: self.debug,
            signatures: self.signatures,
            portable_mir: self.portable_mir,
        })
    }
}

/// File-local VerificationMetadata facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct VerificationMetadataRecord {
    bytecode_verified: bool,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    function_layouts: Vec<FunctionLayoutMetadataRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    function_effects: FunctionEffectBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    control_flow_targets: ControlFlowTargetMetadataBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    typed_path_fingerprints: PathFingerprintBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    public_abi_fingerprints: PublicAbiFingerprintBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    dependency_fingerprints: DependencyFingerprintBuffer,
    host_interface_fingerprint: ArtifactFingerprint,
    loader: LoaderValidationMetadata,
}

impl VerificationMetadataRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &VerificationMetadata,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            bytecode_verified: value.bytecode_verified,
            function_layouts: value
                .function_layouts
                .iter()
                .map(|value| FunctionLayoutMetadataRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            function_effects: value.function_effects.clone(),
            control_flow_targets: value.control_flow_targets.clone(),
            typed_path_fingerprints: value.typed_path_fingerprints.clone(),
            public_abi_fingerprints: value.public_abi_fingerprints.clone(),
            dependency_fingerprints: value.dependency_fingerprints.clone(),
            host_interface_fingerprint: value.host_interface_fingerprint,
            loader: value.loader.clone(),
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<VerificationMetadata, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(VerificationMetadata {
            bytecode_verified: self.bytecode_verified,
            function_layouts: self
                .function_layouts
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            function_effects: self.function_effects,
            control_flow_targets: self.control_flow_targets,
            typed_path_fingerprints: self.typed_path_fingerprints,
            public_abi_fingerprints: self.public_abi_fingerprints,
            dependency_fingerprints: self.dependency_fingerprints,
            host_interface_fingerprint: self.host_interface_fingerprint,
            loader: self.loader,
        })
    }
}

/// File-local FunctionLayoutMetadata facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct FunctionLayoutMetadataRecord {
    semantic: SemanticSlotsRecord,
    function: FunctionRef,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    params: Vec<ValueType>,
    return_type: ValueType,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    locals: Vec<ValueType>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    registers: Vec<ValueType>,
    roots: RootSlotLayout,
}

impl FunctionLayoutMetadataRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &FunctionLayoutMetadata,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            semantic: SemanticSlotsRecord::encode(&value.semantic, encoding, cancel)?,
            function: value.function,
            params: value.params.clone(),
            return_type: value.return_type,
            locals: value.locals.clone(),
            registers: value.registers.clone(),
            roots: value.roots.clone(),
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<FunctionLayoutMetadata, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(FunctionLayoutMetadata {
            semantic: self.semantic.decode(decoding, cancel)?,
            function: self.function,
            params: self.params,
            return_type: self.return_type,
            locals: self.locals,
            registers: self.registers,
            roots: self.roots,
        })
    }
}
