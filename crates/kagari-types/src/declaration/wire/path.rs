//! Bounded file-local records for common typed manual path declarations.
use crate::{
    declaration::host::PathAccess,
    declaration::path::{
        HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
        HostVirtualSegmentDeclaration,
    },
    decode_limits,
    ty::wire::WireType,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct PathDeclarationRecord {
    root: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::nested")]
    segments: Vec<WireHostPathSegmentDeclaration>,
    access: PathAccess,
    schema_epoch: u64,
}

impl PathDeclarationRecord {
    /// Encode bounded shared path facts with canonical file-local identities.
    pub fn encode(
        value: &HostPathDeclaration,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(PathDeclarationRecord {
            root: encoding
                .reference(value.root)
                .map_err(DefinitionMappingError::from)?,
            segments: value
                .segments
                .iter()
                .map(|value| WireHostPathSegmentDeclaration::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            access: value.access,
            schema_epoch: value.schema_epoch,
        })
    }
}

impl PathDeclarationRecord {
    /// Decode identities and bounded semantic types without granting execution.
    /// The receiving declaration or executable validator checks schema/access.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostPathDeclaration, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(HostPathDeclaration {
            root: decoding
                .resolve(self.root)
                .map_err(DefinitionMappingError::from)?,
            segments: self
                .segments
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            access: self.access,
            schema_epoch: self.schema_epoch,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct WireHostIndexSegmentDeclaration {
    slot: u32,
    collection: WireType,
    index: WireType,
    result: WireType,
    access: PathAccess,
}

impl WireHostIndexSegmentDeclaration {
    fn encode(
        value: &HostIndexSegmentDeclaration,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(WireHostIndexSegmentDeclaration {
            slot: value.slot,
            collection: WireType::encode(&value.collection, encoding, cancel)
                .map_err(DefinitionMappingError::from)?,
            index: WireType::encode(&value.index, encoding, cancel)
                .map_err(DefinitionMappingError::from)?,
            result: WireType::encode(&value.result, encoding, cancel)
                .map_err(DefinitionMappingError::from)?,
            access: value.access,
        })
    }
}

impl WireHostIndexSegmentDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostIndexSegmentDeclaration, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(HostIndexSegmentDeclaration {
            slot: self.slot,
            collection: self
                .collection
                .decode(decoding, cancel)
                .map_err(DefinitionMappingError::from)?,
            index: self
                .index
                .decode(decoding, cancel)
                .map_err(DefinitionMappingError::from)?,
            result: self
                .result
                .decode(decoding, cancel)
                .map_err(DefinitionMappingError::from)?,
            access: self.access,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct WireHostVirtualSegmentDeclaration {
    name: String,
    result: WireType,
    access: PathAccess,
}

impl WireHostVirtualSegmentDeclaration {
    fn encode(
        value: &HostVirtualSegmentDeclaration,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(WireHostVirtualSegmentDeclaration {
            name: value.name.clone(),
            result: WireType::encode(&value.result, encoding, cancel)
                .map_err(DefinitionMappingError::from)?,
            access: value.access,
        })
    }
}

impl WireHostVirtualSegmentDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostVirtualSegmentDeclaration, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(HostVirtualSegmentDeclaration {
            name: self.name,
            result: self
                .result
                .decode(decoding, cancel)
                .map_err(DefinitionMappingError::from)?,
            access: self.access,
        })
    }
}

#[derive(Serialize, Deserialize)]
enum WireHostPathSegmentDeclaration {
    Field(PortableDefinitionRef),
    Index(WireHostIndexSegmentDeclaration),
    Virtual(WireHostVirtualSegmentDeclaration),
}

impl WireHostPathSegmentDeclaration {
    fn encode(
        value: &HostPathSegmentDeclaration,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            HostPathSegmentDeclaration::Field(id) => WireHostPathSegmentDeclaration::Field(
                encoding
                    .reference(*id)
                    .map_err(DefinitionMappingError::from)?,
            ),
            HostPathSegmentDeclaration::Index(segment) => WireHostPathSegmentDeclaration::Index(
                WireHostIndexSegmentDeclaration::encode(segment, encoding, cancel)?,
            ),
            HostPathSegmentDeclaration::Virtual(segment) => {
                WireHostPathSegmentDeclaration::Virtual(WireHostVirtualSegmentDeclaration::encode(
                    segment, encoding, cancel,
                )?)
            }
        })
    }
}

impl WireHostPathSegmentDeclaration {
    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostPathSegmentDeclaration, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Field(id) => HostPathSegmentDeclaration::Field(
                decoding.resolve(id).map_err(DefinitionMappingError::from)?,
            ),
            Self::Index(segment) => {
                HostPathSegmentDeclaration::Index(segment.decode(decoding, cancel)?)
            }
            Self::Virtual(segment) => {
                HostPathSegmentDeclaration::Virtual(segment.decode(decoding, cancel)?)
            }
        })
    }
}
