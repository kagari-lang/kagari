//! File-local construction requirements; witness trees belong to callable wire evidence.
use crate::callable::construction::evidence::ConstructionRequirement;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_types::{declaration::wire::leaf::NominalTyRecord, ty::wire::WireType};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct ConstructionRequirementRecord {
    receiver: WireType,
    interface: NominalTyRecord,
}

impl ConstructionRequirementRecord {
    pub fn encode(
        value: &ConstructionRequirement,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            receiver: WireType::encode(&value.receiver, encoding, cancel)?,
            interface: NominalTyRecord::encode(&value.interface, encoding, cancel)?,
        })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConstructionRequirement, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(ConstructionRequirement {
            receiver: self.receiver.decode(decoding, cancel)?,
            interface: self.interface.decode(decoding, cancel)?,
        })
    }
}
