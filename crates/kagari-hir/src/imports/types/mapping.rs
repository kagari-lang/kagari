//! Explicit traversal of the owning HIR records.

use crate::identity_mapping::map_hash_entries;
use crate::imports::types::{ImportedTraitMethod, ImportedType, ImportedTypes};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for ImportedType {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedType {
            native_type: self
                .native_type
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            associated_arities: self.associated_arities.clone(),
            id: self.id.remap_definitions(mapper)?,
            declaration: self.declaration.remap_definitions(mapper)?,
            ty: self.ty.remap_definitions(mapper)?,
            trait_methods: map_sequence(&self.trait_methods, |value| {
                (value).remap_definitions(mapper)
            })?,
            associated_types: self.associated_types.clone(),
            supertraits: map_sequence(&self.supertraits, |value| {
                (value).remap_definitions(mapper)
            })?,
        })
    }
}

impl DefinitionReferences for ImportedType {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.id.visit_definition_ids(visit, cancel)?;
        if let Some(value0) = self.native_type.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.declaration.visit_definition_ids(visit, cancel)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        for value0 in &self.trait_methods {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.supertraits {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for ImportedTraitMethod {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedTraitMethod {
            name: self.name.clone(),
            declaration: mapper.reference(&self.declaration)?,
        })
    }
}

impl DefinitionReferences for ImportedTraitMethod {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        Ok(())
    }
}

impl RemapDefinitions for ImportedTypes {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedTypes {
            resolutions: map_hash_entries(
                self.resolutions.len(),
                self.resolutions.iter().map(|(key, value)| {
                    Ok((
                        key.remap_definitions(mapper)?,
                        value.remap_definitions(mapper)?,
                    ))
                }),
            )?,
            nominal_types: map_hash_entries(
                self.nominal_types.len(),
                self.nominal_types.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
            variants: map_hash_entries(
                self.variants.len(),
                self.variants.iter().map(|(key, value)| {
                    Ok((
                        key.remap_definitions(mapper)?,
                        value.remap_definitions(mapper)?,
                    ))
                }),
            )?,
        })
    }
}

impl DefinitionReferences for ImportedTypes {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (key0, value0) in &self.resolutions {
            key0.visit_definition_ids(visit, cancel)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.nominal_types {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.variants {
            key0.visit_definition_ids(visit, cancel)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
