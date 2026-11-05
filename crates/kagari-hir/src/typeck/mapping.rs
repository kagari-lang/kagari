//! Explicit traversal of the owning HIR records.

use std::sync::Arc;

use crate::identity_mapping::map_hash_entries;
use crate::typeck::{ModuleSignatures, TypedModule};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for ModuleSignatures {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ModuleSignatures {
            statics: map_hash_entries(
                self.statics.len(),
                self.statics
                    .iter()
                    .map(|(key, value)| Ok((*key, value.remap_definitions(mapper)?))),
            )?,
            type_bounds: map_hash_entries(
                self.type_bounds.len(),
                self.type_bounds.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        map_hash_entries(
                            (value).len(),
                            (value).iter().map(|(key, value)| {
                                Ok((
                                    (key).remap_definitions(mapper)?,
                                    map_sequence(value, |value| (value).remap_definitions(mapper))?,
                                ))
                            }),
                        )?,
                    ))
                }),
            )?,
            functions: map_sequence(self.functions.as_slice(), |value| {
                (value).remap_definitions(mapper)
            })?
            .into_iter()
            .collect(),
            type_table: self.type_table.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for ModuleSignatures {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for ty in self.statics.values() {
            ty.visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.type_bounds {
            check_cancel(cancel)?;
            visit(key0)?;
            for (key1, value1) in value0 {
                (key1).visit_definition_ids(visit, cancel)?;
                for value2 in value1 {
                    (value2).visit_definition_ids(visit, cancel)?;
                }
            }
        }
        for value0 in &self.functions {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.type_table.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for TypedModule {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypedModule {
            valid_bodies: self.valid_bodies.clone(),
            valid_static_initializers: self.valid_static_initializers.clone(),
            checked_bodies: self.checked_bodies,
            reused_bodies: self.reused_bodies,
            signatures: Arc::new(self.signatures.remap_definitions(mapper)?),
            consts: map_hash_entries(
                self.consts.len(),
                self.consts
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            const_values: self.const_values.clone(),
            type_table: self.type_table.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for TypedModule {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.signatures.visit_definition_ids(visit, cancel)?;
        for value0 in self.consts.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.type_table.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
