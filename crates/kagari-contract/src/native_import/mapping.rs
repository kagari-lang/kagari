//! Contextual identity traversal of the owning metadata records.
use crate::native_import::NativeImport;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for NativeImport {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeImport {
            result_adapter: self
                .result_adapter
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            generic: self
                .generic
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            instance: self.instance.remap_definitions(mapper)?,
            binding: mapper.reference(&self.binding)?,
            signature: self.signature.remap_definitions(mapper)?,
            requirements: map_sequence(&self.requirements, |value| {
                (value).remap_definitions(mapper)
            })?,
            callables: map_sequence(&self.callables, |value| (value).remap_definitions(mapper))?,
            host: self
                .host
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
        })
    }
}

impl DefinitionReferences for NativeImport {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.result_adapter.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        if let Some(value0) = self.generic.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.instance.visit_definition_ids(visit, cancel)?;
        check_cancel(cancel)?;
        visit(&self.binding)?;
        self.signature.visit_definition_ids(visit, cancel)?;
        for value0 in &self.requirements {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.callables {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        if let Some(value0) = self.host.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
