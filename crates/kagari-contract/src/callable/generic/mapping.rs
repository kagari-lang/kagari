//! Contextual identity traversal of the owning metadata records.
use crate::callable::generic::GenericBody;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for GenericBody<I> {
    type Rebind<J: DefinitionReference> = GenericBody<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(GenericBody {
            parameters: map_sequence(&self.parameters, |value| (value).map_identities(mapper))?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.parameters {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
