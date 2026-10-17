//! Nominal wire applications and callable implementation selectors.
use crate::{
    callable::CallableImplementation,
    declaration::{conversion::ConversionAdapter, wire::signature::NativeDefaultApplicationRecord},
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
    NativeDefault(NativeDefaultApplicationRecord),
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
            CallableImplementation::NativeDefault(value) => {
                CallableImplementationKind::NativeDefault(NativeDefaultApplicationRecord::encode(
                    value, encoding, cancel,
                )?)
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
            CallableImplementationKind::NativeDefault(value) => {
                CallableImplementation::NativeDefault(value.decode(decoding, cancel)?)
            }
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) enum ConversionAdapterRecord {
    Reverse {
        origin: PortableDefinitionRef,
        method: PortableDefinitionRef,
        error: Option<(PortableDefinitionRef, PortableDefinitionRef)>,
        result: Option<PortableDefinitionRef>,
    },
    Forward {
        method: PortableDefinitionRef,
        error: PortableDefinitionRef,
        result: PortableDefinitionRef,
    },
}

impl ConversionAdapterRecord {
    pub(super) fn encode(
        value: &ConversionAdapter,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            ConversionAdapter::Reverse {
                origin,
                method,
                error,
                result,
            } => Self::Reverse {
                origin: encoding.reference(*origin)?,
                method: encoding.reference(*method)?,
                error: error
                    .map(|(source, target)| {
                        Ok::<_, DefinitionMappingError>((
                            encoding.reference(source)?,
                            encoding.reference(target)?,
                        ))
                    })
                    .transpose()?,
                result: result.map(|id| encoding.reference(id)).transpose()?,
            },
            ConversionAdapter::Forward {
                method,
                error,
                result,
            } => Self::Forward {
                method: encoding.reference(*method)?,
                error: encoding.reference(*error)?,
                result: encoding.reference(*result)?,
            },
        })
    }

    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConversionAdapter, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Reverse {
                origin,
                method,
                error,
                result,
            } => ConversionAdapter::Reverse {
                origin: decoding.resolve(origin)?,
                method: decoding.resolve(method)?,
                error: error
                    .map(|(source, target)| {
                        Ok::<_, DefinitionMappingError>((
                            decoding.resolve(source)?,
                            decoding.resolve(target)?,
                        ))
                    })
                    .transpose()?,
                result: result.map(|id| decoding.resolve(id)).transpose()?,
            },
            Self::Forward {
                method,
                error,
                result,
            } => ConversionAdapter::Forward {
                method: decoding.resolve(method)?,
                error: decoding.resolve(error)?,
                result: decoding.resolve(result)?,
            },
        })
    }
}
