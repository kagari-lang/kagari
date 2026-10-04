use crate::declaration::requirement::NativeCallableRequirement;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for NativeCallableRequirement<I> {
    type Rebind<J: DefinitionReference> = NativeCallableRequirement<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeCallableRequirement {
            receiver: self.receiver.map_identities(mapper)?,
            interface: self.interface.map_identities(mapper)?,
            member: mapper.reference(&self.member)?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.receiver.visit_definitions(visit, cancel)?;
        self.interface.visit_definitions(visit, cancel)?;
        check_cancel(cancel)?;
        visit(&self.member)?;
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
