//! KBC file records and canonical fingerprints share one explicit ID conversion.
mod calls;
mod envelope;
mod function;
mod instruction;
mod interface;
mod module;

use crate::{
    artifact::{
        KbcArtifact,
        wire::{
            envelope::KbcArtifactRecord,
            module::{BytecodeModuleRecord, BytecodeProgramRecord},
        },
    },
    module::BytecodeModule,
    program::BytecodeProgram,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        metadata::DefinitionMetadata,
        scoped::DefinitionReferences,
        table::{
            DefinitionTable,
            wire::{DefinitionEncoding, PortableDefinitionTable},
        },
    },
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Exact identity scope plus unverified artifact facts; no execution seal is encoded.
#[derive(Serialize, Deserialize)]
pub(super) struct ArtifactEnvelope {
    records: KbcArtifactRecord,
    definitions: PortableDefinitionTable,
}

impl ArtifactEnvelope {
    pub(super) fn decode(
        self,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<KbcArtifact>, DefinitionMappingError> {
        check_cancel(cancel)?;
        let decoding = self.definitions.decode()?;
        let records = self.records.decode(&decoding, cancel)?;
        DefinitionMetadata::checked(decoding.table, records, cancel)
    }
}

fn encoding(
    record: &impl DefinitionReferences,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<DefinitionEncoding, DefinitionMappingError> {
    check_cancel(cancel)?;
    let mut references = HashSet::new();
    record.visit_definition_ids(
        &mut |id| {
            references.insert(*id);
            Ok(())
        },
        cancel,
    )?;
    Ok(definitions.encode(references)?)
}

pub(super) fn encode_artifact(
    artifact: &KbcArtifact,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<ArtifactEnvelope, DefinitionMappingError> {
    let encoding = encoding(artifact, definitions, cancel)?;
    let records = KbcArtifactRecord::encode(artifact, &encoding, cancel)?;
    Ok(ArtifactEnvelope {
        records,
        definitions: encoding.table,
    })
}

pub(super) fn encode_program(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(BytecodeProgramRecord, PortableDefinitionTable), DefinitionMappingError> {
    let encoding = encoding(program, definitions, cancel)?;
    let records = BytecodeProgramRecord::encode(program, &encoding, cancel)?;
    Ok((records, encoding.table))
}

pub(super) fn encode_module(
    module: &BytecodeModule,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(BytecodeModuleRecord, PortableDefinitionTable), DefinitionMappingError> {
    let encoding = encoding(module, definitions, cancel)?;
    let records = BytecodeModuleRecord::encode(module, &encoding, cancel)?;
    Ok((records, encoding.table))
}
