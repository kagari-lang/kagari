//! Contextual identity traversal of the owning metadata records.
use crate::callable::interface::{InterfaceCallContract, InterfaceMethodSignature};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceCallContract<I> {
    type Rebind<J: DefinitionReference> = InterfaceCallContract<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceCallContract {
            receiver: self
                .receiver
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            operations: map_sequence(&self.operations, |value| (value).map_identities(mapper))?,
            interface: self.interface.map_identities(mapper)?,
            method_slot: self.method_slot,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.receiver.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.operations {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.interface.visit_definitions(visit, cancel)?;
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceMethodSignature<I> {
    type Rebind<J: DefinitionReference> = InterfaceMethodSignature<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceMethodSignature {
            params: map_sequence(&self.params, |value| (value).map_identities(mapper))?,
            result: self.result.map_identities(mapper)?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.params {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.result.visit_definitions(visit, cancel)?;
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
