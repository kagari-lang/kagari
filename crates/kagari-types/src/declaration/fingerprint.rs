//! Canonical fingerprints for shared schemas and typed path member contracts.
use crate::{
    declaration::{TypeDeclaration, TypeDef, wire::declarations::TypeDeclarationRecord},
    ty::{Ty, wire::WireType},
};
use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

/// Fingerprint a schema with its declaring identity and canonical reference table.
/// Runtime slots, Rust layout and pointer identities never participate. Returns
/// identity, bounded shape, cancellation or encoding errors.
pub fn schema(
    owner: DefinitionId,
    definition: &TypeDef,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<u64, DefinitionMappingError> {
    let declaration = TypeDeclaration {
        declaration: owner,
        definition: definition.clone(),
    };
    let mut references = BTreeSet::new();
    declaration.visit_definition_ids(
        &mut |id| {
            references.insert(*id);
            Ok(())
        },
        cancel,
    )?;
    let encoding = definitions.encode(references)?;
    let record = TypeDeclarationRecord::encode(&declaration, &encoding, cancel)?;
    canonical(&(record, encoding.table))
}

/// Fingerprint one checked field after its owner has applied generic arguments.
/// The owner and remaining schema facts retain the field's nominal provenance.
/// Returns the schema errors above or an invalid field slot.
pub fn field(
    owner: DefinitionId,
    definition: &TypeDef,
    slot: usize,
    ty: &Ty,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<u64, DefinitionMappingError> {
    let mut selected = definition
        .fields
        .get(slot)
        .cloned()
        .ok_or(DefinitionMappingError::InvalidContract)?;
    selected.ty = ty.clone();
    let mut declaration = definition.clone();
    declaration.fields = vec![selected];
    schema(owner, &declaration, definitions, cancel)
}

/// Fingerprint a semantic value type without substituting a storage-specific DSL.
/// All references use the same canonical scoped encoding as declaration schemas.
pub fn value(
    ty: &Ty,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<u64, DefinitionMappingError> {
    let mut references = BTreeSet::new();
    ty.visit_definition_ids(
        &mut |id| {
            references.insert(*id);
            Ok(())
        },
        cancel,
    )?;
    let encoding = definitions.encode(references)?;
    let record = WireType::encode(ty, &encoding, cancel)?;
    canonical(&(record, encoding.table))
}

fn canonical(value: &impl Serialize) -> Result<u64, DefinitionMappingError> {
    let mut sink = Fingerprint(0xcbf29ce484222325);
    sink.write_all(b"kagari-canonical-v2")
        .map_err(|_| DefinitionMappingError::InvalidContract)?;
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize_into(&mut sink, value)
        .map_err(|_| DefinitionMappingError::InvalidContract)?;
    Ok(sink.0)
}

struct Fingerprint(u64);

impl Write for Fingerprint {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
