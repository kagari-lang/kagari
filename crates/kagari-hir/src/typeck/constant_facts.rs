//! Checked constant prerequisites reused by imports and function-body analysis.

use std::collections::HashMap;

use crate::{
    DiagnosticBuffer,
    hir::ids::ConstId,
    identity_mapping::map_hash_entries,
    typeck::{scalar::ScalarValue, table::TypeTable},
    types::TypeId,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

/// Facts for one exact source lowering, before checking statics or function bodies.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CheckedConstants {
    pub types: HashMap<ConstId, TypeId>,
    pub values: HashMap<ConstId, ScalarValue>,
    pub table: TypeTable,
    pub diagnostics: DiagnosticBuffer,
}

impl RemapDefinitions for CheckedConstants {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            types: map_hash_entries(
                self.types.len(),
                self.types
                    .iter()
                    .map(|(key, value)| Ok((*key, value.remap_definitions(mapper)?))),
            )?,
            values: self.values.clone(),
            table: self.table.remap_definitions(mapper)?,
            diagnostics: self.diagnostics.clone(),
        })
    }
}

impl DefinitionReferences for CheckedConstants {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        for ty in self.types.values() {
            ty.visit_definition_ids(visit, cancel)?;
        }
        self.table.visit_definition_ids(visit, cancel)
    }
}
