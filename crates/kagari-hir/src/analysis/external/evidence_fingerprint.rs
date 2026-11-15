//! Canonical static-evidence identity, independent of allocation scopes.

use crate::analysis::{error::AnalysisError, external::evidence::ExternalHost};
use bincode::serialize;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::DefinitionMappingError, metadata::DefinitionMetadata,
        scoped::DefinitionReferences, table::wire::DefinitionEncoding,
    },
};
use kagari_types::declaration::{
    TraitDef,
    module::DeclarationError,
    wire::{
        declarations::{TraitDefRecord, TypeDeclarationRecord},
        path::PathDeclarationRecord,
    },
};
use serde::Serialize;

fn encoding<T: DefinitionReferences>(
    metadata: &DefinitionMetadata<T>,
    cancel: &CancellationToken,
) -> Result<DefinitionEncoding, AnalysisError> {
    let mut ids = Vec::new();
    metadata.records().visit_definition_ids(
        &mut |id| {
            ids.push(*id);
            Ok(())
        },
        cancel,
    )?;
    Ok(metadata
        .definitions()
        .encode(ids)
        .map_err(DefinitionMappingError::from)?)
}

fn bytes(value: &impl Serialize) -> Result<Vec<u8>, AnalysisError> {
    serialize(value).map_err(|error| {
        DeclarationError(format!("invalid external evidence encoding: {error}")).into()
    })
}

pub(in crate::analysis) fn trait_bytes(
    metadata: &DefinitionMetadata<TraitDef>,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, AnalysisError> {
    let encoding = encoding(metadata, cancel)?;
    let record = TraitDefRecord::encode(metadata.records(), &encoding, cancel)?;
    bytes(&(encoding.table, record))
}

pub(in crate::analysis) fn host_bytes(
    metadata: &DefinitionMetadata<ExternalHost>,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, AnalysisError> {
    let encoding = encoding(metadata, cancel)?;
    let host = metadata.records();
    let root = TypeDeclarationRecord::encode(&host.root, &encoding, cancel)?;
    let schemas = host
        .schemas
        .iter()
        .map(|schema| TypeDeclarationRecord::encode(schema, &encoding, cancel))
        .collect::<Result<Vec<_>, _>>()?;
    let paths = host
        .paths
        .iter()
        .map(|path| PathDeclarationRecord::encode(path, &encoding, cancel))
        .collect::<Result<Vec<_>, _>>()?;
    bytes(&(encoding.table, root, schemas, paths))
}
