//! Contextual identity traversal of the owning metadata records.
use crate::layout::{EnumLayout, EnumVariantLayout, StructFieldLayout, StructLayout};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for EnumLayout {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(EnumLayout {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
            variants: map_sequence(&self.variants, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for EnumLayout {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.variants {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for EnumVariantLayout {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(EnumVariantLayout {
            reports_failure: self.reports_failure,
            declaration: mapper.reference(&self.declaration)?,
            payload: map_sequence(&self.payload, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for EnumVariantLayout {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.payload {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for StructLayout {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(StructLayout {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
            fields: map_sequence(&self.fields, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for StructLayout {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.fields {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for StructFieldLayout {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(StructFieldLayout {
            visibility: self.visibility,
            declaration: mapper.reference(&self.declaration)?,
            name: self.name.clone(),
            ty: self.ty.remap_definitions(mapper)?,
            mutable: self.mutable,
        })
    }
}

impl DefinitionReferences for StructFieldLayout {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
