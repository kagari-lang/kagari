//! Explicit traversal of the owning HIR records.

use crate::callable::AppliedCallSignature;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for AppliedCallSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(AppliedCallSignature {
            params: map_sequence(&self.params, |value| (value).remap_definitions(mapper))?,
            return_type: self.return_type.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for AppliedCallSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.return_type.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
