//! Contextual identity traversal of the owning metadata records.
use crate::callable::generic::GenericBody;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for GenericBody {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(GenericBody {
            parameters: map_sequence(&self.parameters, |value| (value).remap_definitions(mapper))?,
            bounds: map_sequence(&self.bounds, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for GenericBody {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.parameters {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
