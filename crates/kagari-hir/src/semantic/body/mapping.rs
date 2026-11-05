//! Definition traversal for retained module facts; HIR IDs keep their source arena.

use crate::{identity_mapping::map_hash_entries, semantic::body::ModuleFacts};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use std::sync::Arc;

impl RemapDefinitions for ModuleFacts {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ModuleFacts {
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

impl DefinitionReferences for ModuleFacts {
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
