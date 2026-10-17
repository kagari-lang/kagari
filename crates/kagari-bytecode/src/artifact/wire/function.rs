//! File-local bytecode functions, metadata and callable table entries.
use crate::{
    artifact::wire::instruction::BytecodeInstructionRecord,
    module::{
        BytecodeDebugMetadata, BytecodeFunction, ControlFlowTargetBuffer, FunctionMetadata,
        FunctionRecord, RootSlotLayout, TypeLayoutBuffer,
    },
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_contract::{
    effects::EffectSet, ids::FunctionRef, slots::wire::SemanticSlotsRecord,
    types::wire::module::ConcreteFunctionIdentityRecord,
};
use serde::{Deserialize, Serialize};

/// File-local BytecodeFunction facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct BytecodeFunctionRecord {
    id: FunctionRef,
    identity: Option<ConcreteFunctionIdentityRecord>,
    name: String,
    parameter_count: u16,
    register_count: u16,
    local_count: u16,
    metadata: FunctionMetadataRecord,
    #[serde(deserialize_with = "kagari_contract::decode_limits::instructions")]
    instructions: Vec<BytecodeInstructionRecord>,
}

impl BytecodeFunctionRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &BytecodeFunction,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            id: value.id,
            identity: value
                .identity
                .as_ref()
                .map(|value| ConcreteFunctionIdentityRecord::encode(value, encoding, cancel))
                .transpose()?,
            name: value.name.clone(),
            parameter_count: value.parameter_count,
            register_count: value.register_count,
            local_count: value.local_count,
            metadata: FunctionMetadataRecord::encode(&value.metadata, encoding, cancel)?,
            instructions: value
                .instructions
                .iter()
                .map(|value| BytecodeInstructionRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<BytecodeFunction, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(BytecodeFunction {
            id: self.id,
            identity: self
                .identity
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
            name: self.name,
            parameter_count: self.parameter_count,
            register_count: self.register_count,
            local_count: self.local_count,
            metadata: self.metadata.decode(decoding, cancel)?,
            instructions: self
                .instructions
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local FunctionMetadata facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct FunctionMetadataRecord {
    semantic: SemanticSlotsRecord,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    params: TypeLayoutBuffer,
    return_type: ValueType,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    locals: TypeLayoutBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    registers: TypeLayoutBuffer,
    roots: RootSlotLayout,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    control_flow_targets: ControlFlowTargetBuffer,
    effects: EffectSet,
    debug: BytecodeDebugMetadata,
}

impl FunctionMetadataRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &FunctionMetadata,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            semantic: SemanticSlotsRecord::encode(&value.semantic, encoding, cancel)?,
            params: value.params.clone(),
            return_type: value.return_type,
            locals: value.locals.clone(),
            registers: value.registers.clone(),
            roots: value.roots.clone(),
            control_flow_targets: value.control_flow_targets.clone(),
            effects: value.effects,
            debug: value.debug.clone(),
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<FunctionMetadata, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(FunctionMetadata {
            semantic: self.semantic.decode(decoding, cancel)?,
            params: self.params,
            return_type: self.return_type,
            locals: self.locals,
            registers: self.registers,
            roots: self.roots,
            control_flow_targets: self.control_flow_targets,
            effects: self.effects,
            debug: self.debug,
        })
    }
}

/// File-local FunctionRecord facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct FunctionRecordWire {
    id: FunctionRef,
    identity: Option<ConcreteFunctionIdentityRecord>,
    name: String,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    params: TypeLayoutBuffer,
    return_type: ValueType,
    effects: EffectSet,
}

impl FunctionRecordWire {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &FunctionRecord,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            id: value.id,
            identity: value
                .identity
                .as_ref()
                .map(|value| ConcreteFunctionIdentityRecord::encode(value, encoding, cancel))
                .transpose()?,
            name: value.name.clone(),
            params: value.params.clone(),
            return_type: value.return_type,
            effects: value.effects,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<FunctionRecord, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(FunctionRecord {
            id: self.id,
            identity: self
                .identity
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
            name: self.name,
            params: self.params,
            return_type: self.return_type,
            effects: self.effects,
        })
    }
}
