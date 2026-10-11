//! Owned semantic metadata scopes and explicit ID-to-ID imports.
use crate::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionTable,
    },
};
use std::collections::HashSet;

/// This owns a checked identity scope, not semantic verification evidence.
/// Executable consumers must separately validate the complete record contract.
#[derive(Debug, Clone)]
pub struct DefinitionMetadata<T> {
    definitions: DefinitionTable,
    records: T,
}

impl<T: DefinitionReferences> DefinitionMetadata<T> {
    pub fn checked(
        definitions: DefinitionTable,
        records: T,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        records.visit_definition_ids(
            &mut |id| definitions.resolve(*id).map(|_| ()).map_err(Into::into),
            cancel,
        )?;
        Ok(Self {
            definitions,
            records,
        })
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    pub fn records(&self) -> &T {
        &self.records
    }

    /// Transfer records and their immutable identity scope to a new owner.
    /// This carries no semantic verification evidence.
    pub fn into_parts(self) -> (DefinitionTable, T) {
        (self.definitions, self.records)
    }

    pub fn into_records(self) -> T {
        self.records
    }
}

impl<T: RemapDefinitions> DefinitionMetadata<T> {
    /// Imports referenced definitions and rebuilds reusable records in `context`.
    ///
    /// Preserves exact symbolic identity and validates the resulting prefix. This
    /// creates no semantic or execution evidence. Snapshot-owned source containers
    /// have reference visitors but do not implement this remapping capability.
    ///
    /// Returns identity, cancellation, limit or record-remapping errors without
    /// publishing partially converted records. Interned names may remain on failure.
    pub fn import_into(
        &self,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<T>, DefinitionMappingError> {
        let mut references = HashSet::new();
        self.records.visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            cancel,
        )?;
        let remap = context.import(&self.definitions, references)?;
        let records = self.records.remap_definitions(&mut DefinitionMapper::new(
            &mut |id| remap.map(*id).map_err(Into::into),
            cancel,
        ))?;
        DefinitionMetadata::checked(context.snapshot(), records, cancel)
    }
}
