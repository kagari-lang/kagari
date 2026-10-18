//! Stored executable path facts, without rebuilding host schema contracts.

use crate::{
    disassembly::presentation::{physical, quoted},
    module::PathRecord,
};
use std::io::{self, Write};

/// The fingerprint links the admitted path contract, not a declaration vector slot.
/// Do not reconstruct schema contracts merely to render these retained facts.
pub(super) fn write(
    writer: &mut impl Write,
    owner: usize,
    slot: usize,
    record: &PathRecord,
) -> io::Result<()> {
    let PathRecord {
        contract_fingerprint,
        id,
        root_ty,
        result_ty,
        read_only,
        debug_name,
    } = record;
    writeln!(
        writer,
        "  @{owner}:path#{slot} id={} fingerprint=0x{contract_fingerprint:016x} root_physical={} result_physical={} read_only={read_only} name={}",
        id.index(),
        physical(*root_ty),
        physical(*result_ty),
        quoted(debug_name)
    )
}
