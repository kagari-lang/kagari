//! Scoped identity traversal of address-free Host declaration contracts.
use crate::hir::item::adt::HostContract;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use std::sync::Arc;

impl RemapDefinitions for HostContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            declaration: mapper.reference(&self.declaration)?,
            definition: Arc::new(self.definition.remap_definitions(mapper)?),
            paths: self.paths.to_vec().remap_definitions(mapper)?.into(),
            schemas: self.schemas.to_vec().remap_definitions(mapper)?.into(),
        })
    }
}

impl DefinitionReferences for HostContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        visit(&self.declaration)?;
        self.definition.visit_definition_ids(visit, cancel)?;
        for path in self.paths.iter() {
            path.visit_definition_ids(visit, cancel)?;
        }
        for schema in self.schemas.iter() {
            schema.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
