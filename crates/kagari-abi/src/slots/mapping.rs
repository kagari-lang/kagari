//! Contextual identity traversal of the owning metadata records.
use crate::slots::SemanticSlots;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_entries,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for SemanticSlots<I> {
    type Rebind<J: DefinitionReference> = SemanticSlots<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(SemanticSlots {
            generic: self
                .generic
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            protocol_adapter: self
                .protocol_adapter
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            params: map_entries(
                self.params.len(),
                self.params
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            result: self
                .result
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            locals: map_entries(
                self.locals.len(),
                self.locals
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            registers: map_entries(
                self.registers.len(),
                self.registers
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.generic.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        if let Some(value0) = self.protocol_adapter.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.params.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        if let Some(value0) = self.result.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.locals.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.registers.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
