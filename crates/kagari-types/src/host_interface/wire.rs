//! Canonical Host envelopes and fingerprints contain file-local references only.
use crate::host_interface::{
    HostInterface, HostInterfaceError, MAGIC, MAX_BYTES, VERSION, adoption_error, codec, hash,
};
use bincode::Options;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::DefinitionRecord,
        metadata::{DefinitionMetadata, PortableMetadata},
        table::{DefinitionId, DefinitionTable, wire::PortableDefinitionRef},
    },
};
use serde::Serialize;
use std::cmp::Ordering;

pub(super) fn canonical_bytes<T>(
    record: &T,
    definitions: &DefinitionTable,
) -> Result<Vec<u8>, HostInterfaceError>
where
    T: DefinitionRecord<DefinitionId> + Clone,
    T::Rebind<PortableDefinitionRef>: Serialize,
{
    let cancel = CancellationToken::default();
    let scoped = DefinitionMetadata::checked(definitions.clone(), record.clone(), &cancel)
        .map_err(adoption_error)?;
    let portable = scoped.to_portable(&cancel).map_err(adoption_error)?;
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
    T: DefinitionRecord<DefinitionId> + Clone,
    T::Rebind<PortableDefinitionRef>: Serialize,
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
    let portable: PortableMetadata<HostInterface<PortableDefinitionRef>> = codec()
        .deserialize(&bytes[6..])
        .map_err(|_| HostInterfaceError::Encoding)?;
    let scoped = portable
        .decode(&CancellationToken::default())
        .map_err(adoption_error)?;
    scoped.records().validate(scoped.definitions())?;
    Ok(scoped)
}
