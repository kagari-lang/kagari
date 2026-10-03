//! Contextual identity traversal of the owning metadata records.
use crate::callable::witness::{OperationWitness, SharedMethodWitness};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel},
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for OperationWitness<I> {
    type Rebind<J: DefinitionReference> = OperationWitness<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Selected(field0) => {
                OperationWitness::Selected(Box::new(((field0).as_ref()).map_identities(mapper)?))
            }
            Self::SharedMethod(field0) => OperationWitness::SharedMethod(Box::new(
                ((field0).as_ref()).map_identities(mapper)?,
            )),
            Self::Forward(field0) => {
                OperationWitness::Forward(Box::new(((field0).as_ref()).map_identities(mapper)?))
            }
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Selected(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::SharedMethod(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Forward(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for SharedMethodWitness<I> {
    type Rebind<J: DefinitionReference> = SharedMethodWitness<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(SharedMethodWitness {
            requirement: self.requirement.map_identities(mapper)?,
            implementation: self.implementation.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.requirement.visit_definitions(visit, cancel)?;
        self.implementation.visit_definitions(visit, cancel)?;
        Ok(())
    }
}
