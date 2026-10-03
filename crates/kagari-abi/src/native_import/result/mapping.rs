//! Contextual identity traversal of the owning metadata records.
use crate::native_import::result::NativeResultAdapter;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel},
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for NativeResultAdapter<I> {
    type Rebind<J: DefinitionReference> = NativeResultAdapter<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeResultAdapter {
            receiver: self.receiver.map_identities(mapper)?,
            implementation: self.implementation.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.receiver.visit_definitions(visit, cancel)?;
        self.implementation.visit_definitions(visit, cancel)?;
        Ok(())
    }
}
