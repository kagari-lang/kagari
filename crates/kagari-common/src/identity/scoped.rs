//! Reference validation and same-representation remapping of scoped semantic records.

use crate::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        table::DefinitionId,
    },
};

/// Visits every definition reference retained by a semantic record.
///
/// This capability validates identity scope without granting conversion or semantic
/// evidence. Snapshot-owned containers may implement it without being remappable.
pub trait DefinitionReferences {
    /// Visits reference fields and identity keys, propagating cancellation,
    /// traversal limits and errors returned by the supplied scope validator.
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError>;
}

/// Rebuilds independently reusable semantic records in another definition scope.
///
/// Implementations remap all reference fields and identity keys, rejecting collapsed
/// keys. Source revisions, body handles and execution evidence are not converted.
pub trait RemapDefinitions: DefinitionReferences + Sized {
    /// Rebuilds this record using the supplied ID mapping; returns an error for
    /// cancellation, an unmapped reference, a limit or a collapsed unique key.
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError>;
}

// SI06 removes these adapters with the remaining representation-generic owners.
impl<T: DefinitionRecord<DefinitionId>> DefinitionReferences for T {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.visit_definitions(visit, cancel)
    }
}

impl<T: DefinitionRecord<DefinitionId, Rebind<DefinitionId> = T>> RemapDefinitions for T {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        self.map_identities(mapper)
    }
}

impl<T: DefinitionReferences> DefinitionReferences for Vec<T> {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for record in self {
            record.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl<T: RemapDefinitions> RemapDefinitions for Vec<T> {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        map_sequence(self, |record| record.remap_definitions(mapper))
    }
}
