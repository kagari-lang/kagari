//! Private serialized Host path declarations.
use crate::host_interface::{
    HostInterfaceError, decode_limits,
    path::{
        HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
        HostVirtualSegmentDeclaration,
    },
    type_declaration::PathAccess,
    value_type::codec::WireHostValueType,
    wire::{Encode, identity_error},
};
use kagari_common::identity::table::{
    DefinitionId,
    wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostPathDeclaration {
    root: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::members")]
    segments: Vec<WireHostPathSegmentDeclaration>,
    access: PathAccess,
    schema_epoch: u64,
}

impl Encode for HostPathDeclaration<DefinitionId> {
    type Wire = WireHostPathDeclaration;

    fn encode(&self, encoding: &DefinitionEncoding) -> Result<Self::Wire, HostInterfaceError> {
        Ok(WireHostPathDeclaration {
            root: encoding.reference(self.root).map_err(identity_error)?,
            segments: self
                .segments
                .iter()
                .map(|value| value.encode(encoding))
                .collect::<Result<_, _>>()?,
            access: self.access,
            schema_epoch: self.schema_epoch,
        })
    }
}

impl WireHostPathDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
    ) -> Result<HostPathDeclaration<DefinitionId>, HostInterfaceError> {
        Ok(HostPathDeclaration {
            root: decoding.resolve(self.root).map_err(identity_error)?,
            segments: self
                .segments
                .into_iter()
                .map(|value| value.decode(decoding))
                .collect::<Result<_, _>>()?,
            access: self.access,
            schema_epoch: self.schema_epoch,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostIndexSegmentDeclaration {
    slot: u32,
    collection: WireHostValueType,
    index: WireHostValueType,
    result: WireHostValueType,
    access: PathAccess,
}

impl Encode for HostIndexSegmentDeclaration<DefinitionId> {
    type Wire = WireHostIndexSegmentDeclaration;

    fn encode(&self, encoding: &DefinitionEncoding) -> Result<Self::Wire, HostInterfaceError> {
        Ok(WireHostIndexSegmentDeclaration {
            slot: self.slot,
            collection: self.collection.encode(encoding)?,
            index: self.index.encode(encoding)?,
            result: self.result.encode(encoding)?,
            access: self.access,
        })
    }
}

impl WireHostIndexSegmentDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
    ) -> Result<HostIndexSegmentDeclaration<DefinitionId>, HostInterfaceError> {
        Ok(HostIndexSegmentDeclaration {
            slot: self.slot,
            collection: self.collection.decode(decoding)?,
            index: self.index.decode(decoding)?,
            result: self.result.decode(decoding)?,
            access: self.access,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostVirtualSegmentDeclaration {
    name: String,
    result: WireHostValueType,
    access: PathAccess,
}

impl Encode for HostVirtualSegmentDeclaration<DefinitionId> {
    type Wire = WireHostVirtualSegmentDeclaration;

    fn encode(&self, encoding: &DefinitionEncoding) -> Result<Self::Wire, HostInterfaceError> {
        Ok(WireHostVirtualSegmentDeclaration {
            name: self.name.clone(),
            result: self.result.encode(encoding)?,
            access: self.access,
        })
    }
}

impl WireHostVirtualSegmentDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
    ) -> Result<HostVirtualSegmentDeclaration<DefinitionId>, HostInterfaceError> {
        Ok(HostVirtualSegmentDeclaration {
            name: self.name,
            result: self.result.decode(decoding)?,
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

impl Encode for HostPathSegmentDeclaration<DefinitionId> {
    type Wire = WireHostPathSegmentDeclaration;

    fn encode(&self, encoding: &DefinitionEncoding) -> Result<Self::Wire, HostInterfaceError> {
        Ok(match self {
            Self::Field(id) => WireHostPathSegmentDeclaration::Field(
                encoding.reference(*id).map_err(identity_error)?,
            ),
            Self::Index(segment) => {
                WireHostPathSegmentDeclaration::Index(segment.encode(encoding)?)
            }
            Self::Virtual(segment) => {
                WireHostPathSegmentDeclaration::Virtual(segment.encode(encoding)?)
            }
        })
    }
}

impl WireHostPathSegmentDeclaration {
    fn decode(
        self,
        decoding: &DefinitionDecoding,
    ) -> Result<HostPathSegmentDeclaration<DefinitionId>, HostInterfaceError> {
        Ok(match self {
            Self::Field(id) => {
                HostPathSegmentDeclaration::Field(decoding.resolve(id).map_err(identity_error)?)
            }
            Self::Index(segment) => HostPathSegmentDeclaration::Index(segment.decode(decoding)?),
            Self::Virtual(segment) => {
                HostPathSegmentDeclaration::Virtual(segment.decode(decoding)?)
            }
        })
    }
}
