//! Contextual identity traversal of the owning metadata records.
use crate::callable::shared::SharedCall;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for SharedCall {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(SharedCall {
            instance: self.instance.remap_definitions(mapper)?,
            implementation: self.implementation.remap_definitions(mapper)?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
            signature: self.signature.remap_definitions(mapper)?,
            operations: map_sequence(&self.operations, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for SharedCall {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.instance.visit_definition_ids(visit, cancel)?;
        self.implementation.visit_definition_ids(visit, cancel)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.signature.visit_definition_ids(visit, cancel)?;
        for value0 in &self.operations {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
