//! Identity traversal keeps exports in the enclosing Program's retained scope.

use crate::exports::{ExportNamespace, ProgramExports};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl DefinitionReferences for ProgramExports {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.check_shape(cancel)?;
        for namespace in self.namespaces.values() {
            for id in namespace.functions.values() {
                check_cancel(cancel)?;
                visit(id)?;
            }
        }
        if let Some(id) = &self.entry {
            visit(id)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for ProgramExports {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        self.check_shape(mapper.cancellation())?;
        Ok(Self {
            namespaces: self
                .namespaces
                .iter()
                .map(|(identity, namespace)| {
                    mapper.check()?;
                    Ok((
                        identity.clone(),
                        ExportNamespace {
                            modules: namespace.modules.clone(),
                            functions: namespace
                                .functions
                                .iter()
                                .map(|(name, id)| Ok((name.clone(), mapper.reference(id)?)))
                                .collect::<Result<_, DefinitionMappingError>>()?,
                        },
                    ))
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
            entry: self
                .entry
                .as_ref()
                .map(|id| mapper.reference(id))
                .transpose()?,
        })
    }
}
