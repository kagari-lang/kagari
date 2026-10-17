//! Contextual identity traversal of the owning metadata records.
use crate::native_import::result::NativeResultAdapter;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for NativeResultAdapter {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeResultAdapter {
            receiver: self.receiver.remap_definitions(mapper)?,
            implementation: self.implementation.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for NativeResultAdapter {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.receiver.visit_definition_ids(visit, cancel)?;
        self.implementation.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
