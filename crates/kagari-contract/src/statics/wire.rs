//! Portable static records use the enclosing artifact's bounded identity table.
use crate::statics::{StaticContract, StaticDescriptor};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::DefinitionMappingError,
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_types::{ty::wire::WireType, visibility::Visibility};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct StaticContractRecord {
    declaration: PortableDefinitionRef,
    supplier: ModuleIdentity,
    name: String,
    ty: WireType,
    mutable: bool,
    visibility: Visibility,
}

impl StaticContractRecord {
    pub fn encode(
        value: &StaticContract,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            supplier: value.supplier.clone(),
            name: value.name.clone(),
            ty: WireType::encode(&value.ty, encoding, cancel)?,
            mutable: value.mutable,
            visibility: value.visibility,
        })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<StaticContract, DefinitionMappingError> {
        Ok(StaticContract {
            declaration: decoding.resolve(self.declaration)?,
            supplier: self.supplier,
            name: self.name,
            ty: self.ty.decode(decoding, cancel)?,
            mutable: self.mutable,
            visibility: self.visibility,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct StaticDescriptorRecord<I> {
    contract: StaticContractRecord,
    initializer: I,
}

impl<I: Clone> StaticDescriptorRecord<I> {
    pub fn encode(
        value: &StaticDescriptor<I>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            contract: StaticContractRecord::encode(&value.contract, encoding, cancel)?,
            initializer: value.initializer.clone(),
        })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<StaticDescriptor<I>, DefinitionMappingError> {
        Ok(StaticDescriptor {
            contract: self.contract.decode(decoding, cancel)?,
            initializer: self.initializer,
        })
    }
}
