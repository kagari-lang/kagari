//! Contextual identity traversal of the owning metadata records.
use crate::host_interface::{HostFunctionDeclaration, HostInterface, HostParameter};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for HostParameter {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostParameter {
            name: self.name.clone(),
            ty: self.ty.remap_definitions(mapper)?,
            passing: self.passing,
        })
    }
}

impl DefinitionReferences for HostParameter {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for HostFunctionDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostFunctionDeclaration {
            id: mapper.reference(&self.id)?,
            symbol: self.symbol.clone(),
            params: map_sequence(&self.params, |value| (value).remap_definitions(mapper))?,
            return_type: self.return_type.remap_definitions(mapper)?,
            effects: self.effects,
            documentation: self.documentation.clone(),
        })
    }
}

impl DefinitionReferences for HostFunctionDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.id)?;
        for value0 in &self.params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.return_type.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for HostInterface {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostInterface {
            paths: map_sequence(&self.paths, |value| (value).remap_definitions(mapper))?,
            types: map_sequence(&self.types, |value| (value).remap_definitions(mapper))?,
            functions: map_sequence(&self.functions, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for HostInterface {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.paths {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.types {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.functions {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
