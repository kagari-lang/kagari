//! Contextual identity traversal of the owning metadata records.
use crate::callable::shared::SharedCall;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for SharedCall<I> {
    type Rebind<J: DefinitionReference> = SharedCall<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(SharedCall {
            instance: self.instance.map_identities(mapper)?,
            implementation: self.implementation.map_identities(mapper)?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
            signature: self.signature.map_identities(mapper)?,
            operations: map_sequence(&self.operations, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.instance.visit_definitions(visit, cancel)?;
        self.implementation.visit_definitions(visit, cancel)?;
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.signature.visit_definitions(visit, cancel)?;
        for value0 in &self.operations {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
