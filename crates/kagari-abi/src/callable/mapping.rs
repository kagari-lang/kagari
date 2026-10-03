//! Contextual identity traversal of the owning metadata records.
use crate::callable::{CallableImplementation, NativeDefaultApplication};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for CallableImplementation<I> {
    type Rebind<J: DefinitionReference> = CallableImplementation<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Required => CallableImplementation::Required,
            Self::Script => CallableImplementation::Script,
            Self::Native(field0) => CallableImplementation::Native(mapper.reference(field0)?),
            Self::NativeDefault(field0) => {
                CallableImplementation::NativeDefault((field0).map_identities(mapper)?)
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
            Self::Required => {}
            Self::Script => {}
            Self::Native(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::NativeDefault(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for NativeDefaultApplication<I> {
    type Rebind<J: DefinitionReference> = NativeDefaultApplication<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeDefaultApplication {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
