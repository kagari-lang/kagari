//! Prepare immutable source associations before candidate publication.
use crate::{
    error::RuntimeError,
    host::HostRegistryId,
    module::{VerifiedProgram, dispatch::identity::PublicationDefinitions},
    source::{DeclarationInfo, ProgramSourceSnapshot, SnapshotData, SourceDeclaration},
};

use std::{collections::HashMap, sync::Arc};

impl ProgramSourceSnapshot {
    pub(crate) fn prepare(
        runtime: HostRegistryId,
        scope: Arc<PublicationDefinitions>,
        program: &VerifiedProgram,
        previous: Option<&Self>,
    ) -> Result<Self, RuntimeError> {
        let package = program.source_package().cloned();
        let records: Vec<_> = match &package {
            Some(package) => package
                .records()
                .declarations
                .iter()
                .map(|record| (record.declaration, Some(record.kind)))
                .collect(),
            None => program
                .update_evidence()
                .sources
                .iter()
                .map(|source| (source.declaration, None))
                .collect(),
        };
        let mut declarations = Vec::with_capacity(records.len());
        let mut indices = HashMap::with_capacity(records.len());
        for (index, (id, _)) in records.iter().enumerate() {
            indices
                .entry(scope.stable_declaration(*id)?)
                .and_modify(|index| *index = None)
                .or_insert(Some(index));
        }
        let mut changes = Vec::with_capacity(records.len());
        for (index, (id, kind)) in records.into_iter().enumerate() {
            let stable = scope.stable_declaration(id)?;
            let path = scope
                .definitions()
                .resolve(id)
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?
                .to_path();
            let owner = scope
                .definitions()
                .parent(id)
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?
                .map(|parent| {
                    scope
                        .definitions()
                        .resolve(parent)
                        .map(|view| view.to_path())
                })
                .transpose()
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let change = previous
                .and_then(|old| {
                    let old_index = (*old.data.indices.get(&stable)?)?;

                    let old_package = old.source_package()?;
                    let new_package = package.as_ref()?;
                    let old_record = &old_package.records().declarations[old_index];
                    let new_record = &new_package.records().declarations[index];
                    let old_text =
                        &old_package.records().documents[old_record.document as usize].text;
                    let new_text =
                        &new_package.records().documents[new_record.document as usize].text;
                    (old_record.kind == new_record.kind
                        && old_text[old_record.range.start..old_record.range.end]
                            == new_text[new_record.range.start..new_record.range.end])
                        .then_some(old.data.changes[old_index])
                })
                .unwrap_or(scope.publication_token());

            changes.push(change);
            declarations.push(SourceDeclaration {
                info: Arc::new(DeclarationInfo {
                    runtime,
                    publication: scope.publication_token(),
                    stable,
                    index,
                    ambiguous: indices[&stable].is_none(),
                    path,
                    owner,
                    kind,
                }),
            });
        }
        let associations = package
            .as_ref()
            .map(|package| {
                package
                    .records()
                    .executables
                    .iter()
                    .map(|record| (record.target.clone(), record.clone()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            data: Arc::new(SnapshotData {
                runtime,
                scope,
                package,
                declarations,
                indices,
                changes,
                associations,
            }),
        })
    }
}
