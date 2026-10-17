//! Contextual identity traversal of the owning metadata records.
use crate::native_import::callables::NativeCallableApplication;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for NativeCallableApplication {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeCallableApplication {
            origin: self.origin.clone(),
            requirement: self.requirement.remap_definitions(mapper)?,
            instance: self.instance.remap_definitions(mapper)?,
            implementation: self.implementation.remap_definitions(mapper)?,
            signature: self.signature.remap_definitions(mapper)?,
            effects: self.effects,
        })
    }
}

impl DefinitionReferences for NativeCallableApplication {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.requirement.visit_definition_ids(visit, cancel)?;
        self.instance.visit_definition_ids(visit, cancel)?;
        self.implementation.visit_definition_ids(visit, cancel)?;
        self.signature.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
