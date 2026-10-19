//! File-local semantic slot types and generic callable facts.
use crate::module::BytecodeSemanticSlots;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_contract::callable::wire::GenericBodyRecord;
use kagari_types::{
    declaration::wire::native::NativeCallableRequirementRecord,
    ty::{Ty, wire::WireType},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// File-local BytecodeSemanticSlots facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct BytecodeSemanticSlotsRecord {
    generic: Option<GenericBodyRecord>,
    protocol_adapter: Option<NativeCallableRequirementRecord>,
    #[serde(deserialize_with = "kagari_types::decode_limits::map")]
    params: BTreeMap<usize, WireType>,
    result: Option<WireType>,
    #[serde(deserialize_with = "kagari_types::decode_limits::map")]
    slots: BTreeMap<usize, WireType>,
}

impl BytecodeSemanticSlotsRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &BytecodeSemanticSlots,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            generic: value
                .generic
                .as_ref()
                .map(|value| GenericBodyRecord::encode(value, encoding, cancel))
                .transpose()?,
            protocol_adapter: value
                .protocol_adapter
                .as_ref()
                .map(|value| NativeCallableRequirementRecord::encode(value, encoding, cancel))
                .transpose()?,
            params: encode_slots(&value.params, encoding, cancel)?,
            result: value
                .result
                .as_ref()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .transpose()?,
            slots: encode_slots(&value.slots, encoding, cancel)?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<BytecodeSemanticSlots, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(BytecodeSemanticSlots {
            generic: self
                .generic
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
            protocol_adapter: self
                .protocol_adapter
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
            params: decode_slots(self.params, decoding, cancel)?,
            result: self
                .result
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .transpose()?,
            slots: decode_slots(self.slots, decoding, cancel)?,
        })
    }
}

// Numeric slot keys do not change identity scope; bounded map decoding rejects duplicates.
fn encode_slots(
    values: &BTreeMap<usize, Ty>,
    encoding: &DefinitionEncoding,
    cancel: &CancellationToken,
) -> Result<BTreeMap<usize, WireType>, DefinitionMappingError> {
    values
        .iter()
        .map(|(&key, value)| Ok((key, WireType::encode(value, encoding, cancel)?)))
        .collect()
}

fn decode_slots(
    values: BTreeMap<usize, WireType>,
    decoding: &DefinitionDecoding,
    cancel: &CancellationToken,
) -> Result<BTreeMap<usize, Ty>, DefinitionMappingError> {
    values
        .into_iter()
        .map(|(key, value)| Ok((key, value.decode(decoding, cancel)?)))
        .collect()
}
