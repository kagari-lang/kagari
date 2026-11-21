//! Nominal wire applications and callable implementation selectors.
use crate::{
    callable::CallableImplementation,
    declaration::conversion::ConversionAdapter,
    decode_limits,
    ty::{NominalTy, wire::WireType},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub struct NominalTyRecord {
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::nested")]
    arguments: Vec<WireType>,
    #[serde(deserialize_with = "decode_limits::map")]
    associated_types: BTreeMap<PortableDefinitionRef, WireType>,
}

impl NominalTyRecord {
    pub fn encode(
        value: &NominalTy,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        let mut associated_types = BTreeMap::new();
        for (member, ty) in &value.associated_types {
            let member = encoding.reference(*member)?;
            let ty = WireType::encode(ty, encoding, cancel)?;
            if associated_types.insert(member, ty).is_some() {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            arguments: value
                .arguments
                .iter()
                .map(|ty| WireType::encode(ty, encoding, cancel))
                .collect::<Result<_, _>>()?,
            associated_types,
        })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<NominalTy, DefinitionMappingError> {
        check_cancel(cancel)?;
        let mut associated_types = BTreeMap::new();
        for (member, ty) in self.associated_types {
            let member = decoding.resolve(member)?;
            let ty = ty.decode(decoding, cancel)?;
            if associated_types.insert(member, ty).is_some() {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
        Ok(NominalTy {
            declaration: decoding.resolve(self.declaration)?,
            arguments: self
                .arguments
                .into_iter()
                .map(|ty| ty.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            associated_types,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct CallableImplementationRecord(CallableImplementationKind);

#[derive(Serialize, Deserialize)]
enum CallableImplementationKind {
    Required,
    Script,
    Native(PortableDefinitionRef),
}

impl CallableImplementationRecord {
    pub fn encode(
        value: &CallableImplementation,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self(match value {
            CallableImplementation::Required => CallableImplementationKind::Required,
            CallableImplementation::Script => CallableImplementationKind::Script,
            CallableImplementation::Native(id) => {
                CallableImplementationKind::Native(encoding.reference(*id)?)
            }
        }))
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<CallableImplementation, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self.0 {
            CallableImplementationKind::Required => CallableImplementation::Required,
            CallableImplementationKind::Script => CallableImplementation::Script,
            CallableImplementationKind::Native(id) => {
                CallableImplementation::Native(decoding.resolve(id)?)
            }
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct ConversionAdapterRecord {
    method: PortableDefinitionRef,
    error: PortableDefinitionRef,
    result: PortableDefinitionRef,
}

impl ConversionAdapterRecord {
    pub(super) fn encode(
        value: &ConversionAdapter,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            method: encoding.reference(value.method)?,
            error: encoding.reference(value.error)?,
            result: encoding.reference(value.result)?,
        })
    }

    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConversionAdapter, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(ConversionAdapter {
            method: decoding.resolve(self.method)?,
            error: decoding.resolve(self.error)?,
            result: decoding.resolve(self.result)?,
        })
    }
}
