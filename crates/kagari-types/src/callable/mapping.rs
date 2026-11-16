//! Contextual identity traversal of the owning metadata records.
use crate::callable::{CallableImplementation, Signature};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for CallableImplementation {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Required => CallableImplementation::Required,
            Self::Script => CallableImplementation::Script,
            Self::Native(field0) => CallableImplementation::Native(mapper.reference(field0)?),
        })
    }
}

impl DefinitionReferences for CallableImplementation {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Required => {}
            Self::Script => {}
            Self::Native(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for Signature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Signature {
            params: map_sequence(&self.params, |value| (value).remap_definitions(mapper))?,
            result: self.result.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for Signature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.result.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
