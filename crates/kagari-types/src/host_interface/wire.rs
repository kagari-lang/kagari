//! Canonical Host envelopes and fingerprints contain file-local references only.
use crate::host_interface::{
    HostInterface, HostInterfaceError, MAGIC, MAX_BYTES, VERSION, adoption_error, codec,
    decode_limits, hash,
    wire::{
        declarations::{WireHostFunctionDeclaration, WireHostTypeDeclaration},
        path::WireHostPathDeclaration,
    },
};
use bincode::Options;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        metadata::DefinitionMetadata,
        scoped::DefinitionReferences,
        table::{
            DefinitionId, DefinitionTable, DefinitionTableError,
            wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionTable},
        },
    },
};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashSet};

mod declarations;
mod path;

/// This trait only selects an owner's private serialized record; it cannot rebind
/// semantic records to arbitrary identity representations.
pub(super) trait Encode {
    type Wire: Serialize;

    fn encode(&self, encoding: &DefinitionEncoding) -> Result<Self::Wire, HostInterfaceError>;
}

pub(super) fn identity_error(error: DefinitionTableError) -> HostInterfaceError {
    adoption_error(error.into())
}

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    records: T,
    definitions: PortableDefinitionTable,
}

pub(super) fn canonical_bytes<T>(
    record: &T,
    definitions: &DefinitionTable,
) -> Result<Vec<u8>, HostInterfaceError>
where
    T: DefinitionReferences + Encode,
{
    let cancel = CancellationToken::default();
    let mut references = HashSet::new();
    record
        .visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            &cancel,
        )
        .map_err(adoption_error)?;
    let encoding = definitions.encode(references).map_err(identity_error)?;
    let portable = Envelope {
        records: record.encode(&encoding)?,
        definitions: encoding.table,
    };
    codec()
        .serialize(&portable)
        .map_err(|_| HostInterfaceError::Encoding)
}

pub(super) fn fingerprint<T>(
    domain: &[u8],
    record: &T,
    definitions: &DefinitionTable,
) -> Result<u64, HostInterfaceError>
where
    T: DefinitionReferences + Encode,
{
    Ok(hash(
        domain
            .iter()
            .copied()
            .chain(canonical_bytes(record, definitions)?),
    ))
}

pub(super) fn encode(
    interface: &HostInterface<DefinitionId>,
    definitions: &DefinitionTable,
) -> Result<Vec<u8>, HostInterfaceError> {
    let mut canonical = interface.clone();
    let mut invalid = false;
    let mut compare = |left, right| {
        definitions.compare(left, right).unwrap_or_else(|_| {
            invalid = true;
            Ordering::Equal
        })
    };
    canonical
        .types
        .sort_by(|left, right| compare(left.id, right.id));
    canonical
        .functions
        .sort_by(|left, right| compare(left.id, right.id));
    if invalid {
        return Err(HostInterfaceError::InvalidDeclaration);
    }
    let mut paths = canonical
        .paths
        .into_iter()
        .map(|path| Ok((canonical_bytes(&path, definitions)?, path)))
        .collect::<Result<Vec<_>, HostInterfaceError>>()?;
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    canonical.paths = paths.into_iter().map(|(_, path)| path).collect();
    let records = canonical_bytes(&canonical, definitions)?;
    let mut bytes = Vec::with_capacity(6 + records.len());
    bytes.extend(MAGIC);
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend(records);
    if bytes.len() as u64 > MAX_BYTES {
        return Err(HostInterfaceError::TooLarge);
    }
    Ok(bytes)
}

pub(super) fn decode(
    bytes: &[u8],
) -> Result<DefinitionMetadata<HostInterface<DefinitionId>>, HostInterfaceError> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(HostInterfaceError::TooLarge);
    }
    if bytes.get(..4) != Some(MAGIC.as_slice())
        || bytes.get(4..6) != Some(VERSION.to_le_bytes().as_slice())
    {
        return Err(HostInterfaceError::Version);
    }
    let portable: Envelope<WireHostInterface> = codec()
        .deserialize(&bytes[6..])
        .map_err(|_| HostInterfaceError::Encoding)?;
    let decoding = portable.definitions.decode().map_err(identity_error)?;
    let records = portable.records.decode(&decoding)?;
    records.validate(&decoding.table)?;
    DefinitionMetadata::checked(decoding.table, records, &CancellationToken::default())
        .map_err(adoption_error)
}

#[derive(Serialize, Deserialize)]
pub(super) struct WireHostInterface {
    #[serde(deserialize_with = "decode_limits::declarations")]
    paths: Vec<WireHostPathDeclaration>,
    #[serde(deserialize_with = "decode_limits::declarations")]
    types: Vec<WireHostTypeDeclaration>,
    #[serde(deserialize_with = "decode_limits::declarations")]
    functions: Vec<WireHostFunctionDeclaration>,
}

impl Encode for HostInterface<DefinitionId> {
    type Wire = WireHostInterface;

    fn encode(&self, encoding: &DefinitionEncoding) -> Result<Self::Wire, HostInterfaceError> {
        Ok(WireHostInterface {
            paths: self
                .paths
                .iter()
                .map(|value| value.encode(encoding))
                .collect::<Result<_, _>>()?,
            types: self
                .types
                .iter()
                .map(|value| value.encode(encoding))
                .collect::<Result<_, _>>()?,
            functions: self
                .functions
                .iter()
                .map(|value| value.encode(encoding))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl WireHostInterface {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
    ) -> Result<HostInterface<DefinitionId>, HostInterfaceError> {
        Ok(HostInterface {
            paths: self
                .paths
                .into_iter()
                .map(|value| value.decode(decoding))
                .collect::<Result<_, _>>()?,
            types: self
                .types
                .into_iter()
                .map(|value| value.decode(decoding))
                .collect::<Result<_, _>>()?,
            functions: self
                .functions
                .into_iter()
                .map(|value| value.decode(decoding))
                .collect::<Result<_, _>>()?,
        })
    }
}
