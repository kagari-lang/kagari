//! Private serialized Host path declarations.
use crate::ty::wire::WireType;
use crate::{
    declaration::host::PathAccess,
    host_interface::{
        HostInterfaceError, adoption_error, decode_limits,
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            HostVirtualSegmentDeclaration,
        },
        wire::{Encode, check_cancel, identity_error},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct WireHostPathDeclaration {
    root: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::members")]
    segments: Vec<WireHostPathSegmentDeclaration>,
    access: PathAccess,
    schema_epoch: u64,
}

impl Encode for HostPathDeclaration {
    type Wire = WireHostPathDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostPathDeclaration {
            root: encoding.reference(self.root).map_err(identity_error)?,
            segments: self
                .segments
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            access: self.access,
            schema_epoch: self.schema_epoch,
        })
    }
}

impl WireHostPathDeclaration {
    /// Encode scoped facts with the enclosing file table and cancellation token.
    /// Returns reference, bounded shape or cancellation errors without granting
    /// Host validation or execution evidence.
    pub fn encode(
        value: &HostPathDeclaration,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, HostInterfaceError> {
        value.encode(encoding, cancel)
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostPathDeclaration, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostPathDeclaration {
            root: decoding.resolve(self.root).map_err(identity_error)?,
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
pub(in crate::host_interface) struct WireHostIndexSegmentDeclaration {
    slot: u32,
    collection: WireType,
    index: WireType,
    result: WireType,
    access: PathAccess,
}

impl Encode for HostIndexSegmentDeclaration {
    type Wire = WireHostIndexSegmentDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostIndexSegmentDeclaration {
            slot: self.slot,
            collection: WireType::encode(&self.collection, encoding, cancel)
                .map_err(|error| adoption_error(error.into()))?,
            index: WireType::encode(&self.index, encoding, cancel)
                .map_err(|error| adoption_error(error.into()))?,
            result: WireType::encode(&self.result, encoding, cancel)
                .map_err(|error| adoption_error(error.into()))?,
            access: self.access,
        })
    }
}

impl WireHostIndexSegmentDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostIndexSegmentDeclaration, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostIndexSegmentDeclaration {
            slot: self.slot,
            collection: self
                .collection
                .decode(decoding, cancel)
                .map_err(|error| adoption_error(error.into()))?,
            index: self
                .index
                .decode(decoding, cancel)
                .map_err(|error| adoption_error(error.into()))?,
            result: self
                .result
                .decode(decoding, cancel)
                .map_err(|error| adoption_error(error.into()))?,
            access: self.access,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostVirtualSegmentDeclaration {
    name: String,
    result: WireType,
    access: PathAccess,
}

impl Encode for HostVirtualSegmentDeclaration {
    type Wire = WireHostVirtualSegmentDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostVirtualSegmentDeclaration {
            name: self.name.clone(),
            result: WireType::encode(&self.result, encoding, cancel)
                .map_err(|error| adoption_error(error.into()))?,
            access: self.access,
        })
    }
}

impl WireHostVirtualSegmentDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostVirtualSegmentDeclaration, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostVirtualSegmentDeclaration {
            name: self.name,
            result: self
                .result
                .decode(decoding, cancel)
                .map_err(|error| adoption_error(error.into()))?,
            access: self.access,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) enum WireHostPathSegmentDeclaration {
    Field(PortableDefinitionRef),
    Index(WireHostIndexSegmentDeclaration),
    Virtual(WireHostVirtualSegmentDeclaration),
}

impl Encode for HostPathSegmentDeclaration {
    type Wire = WireHostPathSegmentDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Field(id) => WireHostPathSegmentDeclaration::Field(
                encoding.reference(*id).map_err(identity_error)?,
            ),
            Self::Index(segment) => {
                WireHostPathSegmentDeclaration::Index(segment.encode(encoding, cancel)?)
            }
            Self::Virtual(segment) => {
                WireHostPathSegmentDeclaration::Virtual(segment.encode(encoding, cancel)?)
            }
        })
    }
}

impl WireHostPathSegmentDeclaration {
    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostPathSegmentDeclaration, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Field(id) => {
                HostPathSegmentDeclaration::Field(decoding.resolve(id).map_err(identity_error)?)
            }
            Self::Index(segment) => {
                HostPathSegmentDeclaration::Index(segment.decode(decoding, cancel)?)
            }
            Self::Virtual(segment) => {
                HostPathSegmentDeclaration::Virtual(segment.decode(decoding, cancel)?)
            }
        })
    }
}
