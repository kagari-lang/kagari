//! Owned metadata contexts and explicit portable projections.
#[cfg(test)]
mod tests;
use crate::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath,
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
        table::{
            DefinitionId, DefinitionTable, DefinitionTableBuilder,
            wire::{PortableDefinitionRef, PortableDefinitionTable},
        },
    },
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// This owns a checked identity scope, not semantic verification evidence.
/// Executable consumers must separately validate the complete record contract.
#[derive(Debug, Clone)]
pub struct DefinitionMetadata<T> {
    definitions: DefinitionTable,
    records: T,
}

impl<T: DefinitionRecord<DefinitionId>> DefinitionMetadata<T> {
    pub fn checked(
        definitions: DefinitionTable,
        records: T,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        records.visit_definitions(
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

    pub fn into_records(self) -> T {
        self.records
    }

    pub fn to_paths(
        &self,
        cancel: &CancellationToken,
    ) -> Result<T::Rebind<DefinitionPath>, DefinitionMappingError> {
        self.records.map_identities(&mut DefinitionMapper::new(
            &mut |id| Ok(self.definitions.resolve(*id)?.to_path()),
            cancel,
        ))
    }

    pub fn to_portable(
        &self,
        cancel: &CancellationToken,
    ) -> Result<PortableMetadata<T::Rebind<PortableDefinitionRef>>, DefinitionMappingError> {
        let mut references = HashSet::new();
        self.records.visit_definitions(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            cancel,
        )?;
        let encoding = self.definitions.encode(references)?;
        let records = self.records.map_identities(&mut DefinitionMapper::new(
            &mut |id| encoding.reference(*id).map_err(Into::into),
            cancel,
        ))?;
        Ok(PortableMetadata {
            definitions: encoding.table,
            records,
        })
    }

    pub fn import_into(
        &self,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<T::Rebind<DefinitionId>>, DefinitionMappingError>
    where
        T::Rebind<DefinitionId>: DefinitionRecord<DefinitionId>,
    {
        let mut references = HashSet::new();
        self.records.visit_definitions(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            cancel,
        )?;
        let remap = context.import(&self.definitions, references)?;
        let records = self.records.map_identities(&mut DefinitionMapper::new(
            &mut |id| remap.map(*id).map_err(Into::into),
            cancel,
        ))?;
        DefinitionMetadata::checked(context.snapshot(), records, cancel)
    }
}

pub fn scope_record<T: DefinitionRecord<DefinitionPath>>(
    records: &T,
    cancel: &CancellationToken,
) -> Result<DefinitionMetadata<T::Rebind<DefinitionId>>, DefinitionMappingError>
where
    T::Rebind<DefinitionId>: DefinitionRecord<DefinitionId>,
{
    let mut definitions = DefinitionTableBuilder::new()?;
    let records = records.map_identities(&mut DefinitionMapper::new(
        &mut |path| definitions.intern_path(path).map_err(Into::into),
        cancel,
    ))?;
    DefinitionMetadata::checked(definitions.freeze(), records, cancel)
}

/// Serialization carries only a bounded exact table and context-local references.
/// Process table numbers and immutable verification seals cannot enter this format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableMetadata<T> {
    records: T,
    definitions: PortableDefinitionTable,
}

impl<T> PortableMetadata<T> {
    pub fn records(&self) -> &T {
        &self.records
    }
}

impl<T: DefinitionRecord<PortableDefinitionRef>> PortableMetadata<T> {
    pub fn decode(
        self,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<T::Rebind<DefinitionId>>, DefinitionMappingError>
    where
        T::Rebind<DefinitionId>: DefinitionRecord<DefinitionId>,
    {
        let decoded = self.definitions.decode()?;
        let records = self.records.map_identities(&mut DefinitionMapper::new(
            &mut |reference| decoded.resolve(*reference).map_err(Into::into),
            cancel,
        ))?;
        DefinitionMetadata::checked(decoded.table, records, cancel)
    }
}
