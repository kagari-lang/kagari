//! Explicit traversal of the owning HIR records.

use crate::aggregates::implementations::ImplementationSignature;
use crate::identity_mapping::map_hash_entries;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, check_cancel, map_entries, map_sequence,
        },
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for ImplementationSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImplementationSignature {
            engine_owned: self.engine_owned,
            associated_type_families: map_entries(
                self.associated_type_families.len(),
                self.associated_type_families.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
            id: mapper.reference(&self.id)?,
            trait_type: self.trait_type.remap_definitions(mapper)?,
            for_type: self.for_type.remap_definitions(mapper)?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).remap_definitions(mapper)?,
                        map_sequence(value, |value| (value).remap_definitions(mapper))?,
                    ))
                }),
            )?,
            methods: map_entries(
                self.methods.len(),
                self.methods
                    .iter()
                    .map(|(key, value)| Ok((mapper.reference(key)?, mapper.reference(value)?))),
            )?,
        })
    }
}

impl DefinitionReferences for ImplementationSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (key0, value0) in &self.associated_type_families {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.id)?;
        self.trait_type.visit_definition_ids(visit, cancel)?;
        self.for_type.visit_definition_ids(visit, cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definition_ids(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        for (key0, value0) in &self.methods {
            check_cancel(cancel)?;
            visit(key0)?;
            check_cancel(cancel)?;
            visit(value0)?;
        }
        Ok(())
    }
}
