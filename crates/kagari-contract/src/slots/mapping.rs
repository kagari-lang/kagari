//! Contextual identity traversal of the owning metadata records.
use crate::slots::SemanticSlots;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_entries},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for SemanticSlots {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(SemanticSlots {
            generic: self
                .generic
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            protocol_adapter: self
                .protocol_adapter
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            params: map_entries(
                self.params.len(),
                self.params
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            result: self
                .result
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            locals: map_entries(
                self.locals.len(),
                self.locals
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            registers: map_entries(
                self.registers.len(),
                self.registers
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
        })
    }
}

impl DefinitionReferences for SemanticSlots {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.generic.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        if let Some(value0) = self.protocol_adapter.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.params.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        if let Some(value0) = self.result.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.locals.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.registers.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
