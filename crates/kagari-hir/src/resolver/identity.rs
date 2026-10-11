//! Definition references held by resolution inputs; source/Host slot handles stay unchanged.

use crate::{
    imports::ModuleImportFacts,
    resolver::resolved::{DeclarationNames, ResolvedNames},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::DefinitionId,
    },
};

impl DefinitionReferences for ModuleImportFacts {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for id in self.array_interfaces.values() {
            check_cancel(cancel)?;
            visit(id)?;
        }
        Ok(())
    }
}

impl DefinitionReferences for DeclarationNames {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.imports.visit_definition_ids(visit, cancel)?;
        self.hosts.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl DefinitionReferences for ResolvedNames {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.imports.visit_definition_ids(visit, cancel)?;
        self.hosts.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
