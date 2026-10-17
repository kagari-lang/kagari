//! Contextual identity traversal of the owning metadata records.
use crate::callable::witness::{OperationWitness, SharedMethodWitness};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for OperationWitness {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::SharedAdapter(adapter) => {
                OperationWitness::SharedAdapter(Box::new(adapter.remap_definitions(mapper)?))
            }
            Self::Selected(field0) => {
                OperationWitness::Selected(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
            Self::SharedMethod(field0) => OperationWitness::SharedMethod(Box::new(
                ((field0).as_ref()).remap_definitions(mapper)?,
            )),
            Self::Forward(field0) => {
                OperationWitness::Forward(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
        })
    }
}

impl DefinitionReferences for OperationWitness {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::SharedAdapter(adapter) => adapter.visit_definition_ids(visit, cancel)?,
            Self::Selected(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::SharedMethod(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Forward(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for SharedMethodWitness {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(SharedMethodWitness {
            requirement: self.requirement.remap_definitions(mapper)?,
            implementation: self.implementation.remap_definitions(mapper)?,
            operations: self
                .operations
                .iter()
                .map(|operation| operation.remap_definitions(mapper))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl DefinitionReferences for SharedMethodWitness {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.requirement.visit_definition_ids(visit, cancel)?;
        self.implementation.visit_definition_ids(visit, cancel)?;
        for operation in &self.operations {
            operation.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
