//! Contextual identity traversal of the owning metadata records.
use crate::program::BytecodeProgram;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for BytecodeProgram {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(BytecodeProgram {
            root: self.root,
            modules: map_sequence(&self.modules, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for BytecodeProgram {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.modules {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
