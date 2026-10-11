use crate::declaration::requirement::NativeCallableRequirement;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for NativeCallableRequirement {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeCallableRequirement {
            receiver: self.receiver.remap_definitions(mapper)?,
            interface: self.interface.remap_definitions(mapper)?,
            member: mapper.reference(&self.member)?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for NativeCallableRequirement {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.receiver.visit_definition_ids(visit, cancel)?;
        self.interface.visit_definition_ids(visit, cancel)?;
        check_cancel(cancel)?;
        visit(&self.member)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
