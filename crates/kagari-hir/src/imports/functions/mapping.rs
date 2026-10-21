//! Explicit traversal of the owning HIR records.

use crate::identity_mapping::map_hash_entries;
use crate::imports::functions::{ImportedFunction, ImportedFunctions};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for ImportedFunction {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedFunction {
            id: self.id.remap_definitions(mapper)?,
            declaration: mapper.reference(&self.declaration)?,
            site: self.site.remap_definitions(mapper)?,
            signature: self.signature.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for ImportedFunction {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.id.visit_definition_ids(visit, cancel)?;
        visit(&self.declaration)?;
        self.site.visit_definition_ids(visit, cancel)?;
        self.signature.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for ImportedFunctions {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedFunctions {
            functions: map_hash_entries(
                self.functions.len(),
                self.functions.iter().map(|(key, value)| {
                    Ok((
                        key.remap_definitions(mapper)?,
                        value.remap_definitions(mapper)?,
                    ))
                }),
            )?,
            methods: map_hash_entries(
                self.methods.len(),
                self.methods.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
        })
    }
}

impl DefinitionReferences for ImportedFunctions {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (key0, value0) in &self.functions {
            key0.visit_definition_ids(visit, cancel)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.methods {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
