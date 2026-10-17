//! Contextual identity traversal of the owning metadata records.
use crate::callable::interface::{
    InterfaceCallContract, InterfaceMethodSignature, TypeNormalization,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for InterfaceCallContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceCallContract {
            normalizations: map_sequence(&self.normalizations, |fact| {
                Ok(TypeNormalization {
                    source: fact.source.remap_definitions(mapper)?,
                    result: fact.result.remap_definitions(mapper)?,
                })
            })?,
            receiver: self
                .receiver
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            operations: map_sequence(&self.operations, |value| (value).remap_definitions(mapper))?,
            interface: self.interface.remap_definitions(mapper)?,
            method_slot: self.method_slot,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for InterfaceCallContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for fact in &self.normalizations {
            fact.source.visit_definition_ids(visit, cancel)?;
            fact.result.visit_definition_ids(visit, cancel)?;
        }
        if let Some(value0) = self.receiver.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.operations {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.interface.visit_definition_ids(visit, cancel)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for InterfaceMethodSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceMethodSignature {
            has_receiver: self.has_receiver,
            params: map_sequence(&self.params, |value| (value).remap_definitions(mapper))?,
            result: self.result.remap_definitions(mapper)?,
            bounds: map_sequence(&self.bounds, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for InterfaceMethodSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.result.visit_definition_ids(visit, cancel)?;
        for value0 in &self.bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
