//! Nonrecursive wire identities, constraints and callable implementation selectors.
use crate::{
    callable::CallableImplementation,
    declaration::{conversion::ConversionAdapter, wire::signature::NativeDefaultApplicationRecord},
    decode_limits,
    surface::StandardTypeConstraint,
    ty::{Constraint, NominalTy, wire::WireType},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::{
            DefinitionId,
            wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
        },
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub(super) struct NominalTyRecord {
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::nested")]
    arguments: Vec<WireType>,
    #[serde(deserialize_with = "decode_limits::map")]
    associated_types: BTreeMap<PortableDefinitionRef, WireType>,
}

impl NominalTyRecord {
    pub(super) fn encode(
        value: &NominalTy<DefinitionId>,
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

    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<NominalTy<DefinitionId>, DefinitionMappingError> {
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
pub(super) enum ConstraintRecord {
    Standard(StandardTypeConstraint),
    Trait(NominalTyRecord),
}

impl ConstraintRecord {
    pub(super) fn encode(
        value: &Constraint<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            Constraint::Standard(value) => Self::Standard(*value),
            Constraint::Trait(value) => {
                Self::Trait(NominalTyRecord::encode(value, encoding, cancel)?)
            }
        })
    }

    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<Constraint<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Standard(value) => Constraint::Standard(value),
            Self::Trait(value) => Constraint::Trait(value.decode(decoding, cancel)?),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) enum CallableImplementationRecord {
    Required,
    Script,
    Native(PortableDefinitionRef),
    NativeDefault(NativeDefaultApplicationRecord),
}

impl CallableImplementationRecord {
    pub(super) fn encode(
        value: &CallableImplementation<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            CallableImplementation::Required => Self::Required,
            CallableImplementation::Script => Self::Script,
            CallableImplementation::Native(id) => Self::Native(encoding.reference(*id)?),
            CallableImplementation::NativeDefault(value) => Self::NativeDefault(
                NativeDefaultApplicationRecord::encode(value, encoding, cancel)?,
            ),
        })
    }

    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<CallableImplementation<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Required => CallableImplementation::Required,
            Self::Script => CallableImplementation::Script,
            Self::Native(id) => CallableImplementation::Native(decoding.resolve(id)?),
            Self::NativeDefault(value) => {
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
        value: &ConversionAdapter<DefinitionId>,
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
    ) -> Result<ConversionAdapter<DefinitionId>, DefinitionMappingError> {
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
