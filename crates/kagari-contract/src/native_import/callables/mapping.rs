//! Contextual identity traversal of the owning metadata records.
use crate::native_import::callables::NativeCallableApplication;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel},
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for NativeCallableApplication<I> {
    type Rebind<J: DefinitionReference> = NativeCallableApplication<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeCallableApplication {
            origin: self.origin.clone(),
            requirement: self.requirement.map_identities(mapper)?,
            instance: self.instance.map_identities(mapper)?,
            implementation: self.implementation.map_identities(mapper)?,
            signature: self.signature.map_identities(mapper)?,
            effects: self.effects,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.requirement.visit_definitions(visit, cancel)?;
        self.instance.visit_definitions(visit, cancel)?;
        self.implementation.visit_definitions(visit, cancel)?;
        self.signature.visit_definitions(visit, cancel)?;
        Ok(())
    }
}
