//! File-local bytecode functions, metadata and callable table entries.
use crate::{
    artifact::wire::instruction::BytecodeInstructionRecord,
    module::{
        BytecodeDebugMetadata, BytecodeFunction, ControlFlowTargetBuffer, FunctionMetadata,
        FunctionRecord, RootSlotLayout, TypeLayoutBuffer,
    },
};
use crate::{
    artifact::wire::slots::BytecodeSemanticSlotsRecord, instruction::Slot,
    module::InstructionOrigin,
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
    effects::EffectSet, ids::FunctionRef, types::wire::module::ConcreteFunctionIdentityRecord,
};
use serde::{Deserialize, Serialize};

/// File-local BytecodeFunction facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct BytecodeFunctionRecord {
    id: FunctionRef,
    identity: Option<ConcreteFunctionIdentityRecord>,
    name: String,
    parameter_count: u16,
    slot_count: u32,
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
            slot_count: value.slot_count,
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
            slot_count: self.slot_count,
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
    semantic: BytecodeSemanticSlotsRecord,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    params: TypeLayoutBuffer,
    return_type: ValueType,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    slots: TypeLayoutBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    parameter_slots: Vec<Slot>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    origins: Vec<InstructionOrigin>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::instructions")]
    instruction_origins: Vec<usize>,
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
        if !value.origin_selection_valid() {
            return Err(DefinitionMappingError::InvalidContract);
        }
        Ok(Self {
            semantic: BytecodeSemanticSlotsRecord::encode(&value.semantic, encoding, cancel)?,
            params: value.params.clone(),
            return_type: value.return_type,
            slots: value.slots.clone(),
            parameter_slots: value.parameter_slots.clone(),
            origins: value.origins.clone(),
            instruction_origins: value.instruction_origins.clone(),
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
        let metadata = FunctionMetadata {
            semantic: self.semantic.decode(decoding, cancel)?,
            params: self.params,
            return_type: self.return_type,
            slots: self.slots,
            parameter_slots: self.parameter_slots,
            origins: self.origins,
            instruction_origins: self.instruction_origins,
            roots: self.roots,
            control_flow_targets: self.control_flow_targets,
            effects: self.effects,
            debug: self.debug,
        };
        if !metadata.origin_selection_valid() {
            return Err(DefinitionMappingError::InvalidContract);
        }
        Ok(metadata)
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

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::{identity::map::DefinitionContext, span::Span};

    #[test]
    fn source_free_wire_rejects_missing_and_forged_instruction_origin_choices() {
        let context = DefinitionContext::new().unwrap();
        let encoding = context.snapshot().encode([]).unwrap();
        let decoding = encoding.table.decode().unwrap();
        let metadata = FunctionMetadata {
            origins: [0, 0, 1]
                .into_iter()
                .map(|instruction| InstructionOrigin {
                    instruction,
                    span: Span::default(),
                    line: None,
                    column: None,
                })
                .collect(),
            instruction_origins: vec![1],
            ..Default::default()
        };
        for invalid in [vec![], vec![2], vec![1, 1], vec![usize::MAX]] {
            let mut wire =
                FunctionMetadataRecord::encode(&metadata, &encoding, &Default::default()).unwrap();
            // Bypass the checked writer to exercise the source-free reader itself.
            wire.instruction_origins = invalid;
            let bytes = bincode::serialize(&wire).unwrap();
            let decoded: FunctionMetadataRecord = bincode::deserialize(&bytes).unwrap();
            assert!(matches!(
                decoded.decode(&decoding, &Default::default()),
                Err(DefinitionMappingError::InvalidContract)
            ));
        }
    }
}
