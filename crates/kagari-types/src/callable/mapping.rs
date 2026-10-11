//! Contextual identity traversal of the owning metadata records.
use crate::callable::{CallableImplementation, NativeDefaultApplication, Signature};
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
            Self::NativeDefault(field0) => {
                CallableImplementation::NativeDefault((field0).remap_definitions(mapper)?)
            }
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
            Self::NativeDefault(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for NativeDefaultApplication {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeDefaultApplication {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for NativeDefaultApplication {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
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
