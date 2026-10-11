//! Test codecs retain scoped identities and use the ordinary metadata import.
use crate::host_interface::{
    HostInterface, HostInterfaceError, MAGIC, VERSION, adoption_error, wire,
};
use kagari_common::identity::{
    map::DefinitionContext,
    table::{DefinitionId, DefinitionTable},
};

/// Decode and explicitly adopt into the caller's retained fixture context.
pub(super) fn decode_fixture(
    bytes: &[u8],
    context: &DefinitionContext,
) -> Result<HostInterface<DefinitionId>, HostInterfaceError> {
    HostInterface::<DefinitionId>::from_bytes(bytes)?
        .import_into(context, &Default::default())
        .map(|metadata| metadata.into_records())
        .map_err(adoption_error)
}

/// Encode malformed semantic contracts using real file references, bypassing only
/// Host validation so the ordinary decoder can exercise that rejection boundary.
pub(super) fn encode_unchecked_fixture(
    interface: &HostInterface<DefinitionId>,
    definitions: &DefinitionTable,
) -> Result<Vec<u8>, HostInterfaceError> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend(wire::canonical_bytes(interface, definitions)?);
    Ok(bytes)
}
