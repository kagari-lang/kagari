//! Resolution inputs retain their own context-owned module catalog and source units.
//! Publication checks their root/dependency scope. Namespace lookup additionally
//! validates file/revision/arena and membership; source records are not remappable.

use crate::{
    imports::{ModuleImportFacts, catalog::NamespaceCatalog},
    resolver::{
        resolved::{DeclarationNames, ResolvedNames},
        table::NameTable,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::DefinitionId,
    },
};

impl DefinitionReferences for DeclarationNames {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        visit_scope(&self.imports, &self.items, &self.catalog, visit, cancel)
    }
}

impl DefinitionReferences for ResolvedNames {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        visit_scope(&self.imports, &self.items, &self.catalog, visit, cancel)
    }
}

fn visit_scope(
    imports: &ModuleImportFacts,
    items: &NameTable,
    catalog: &NamespaceCatalog,
    visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    check_cancel(cancel)?;
    if imports.scope.unit != items.unit {
        return Err(DefinitionMappingError::InvalidContract);
    }
    if let Some(unit) = &imports.scope.unit {
        if !catalog.valid(unit) {
            return Err(DefinitionMappingError::InvalidContract);
        }
        visit(&unit.module.definition())?;
    }
    for dependency in &imports.dependencies {
        check_cancel(cancel)?;
        catalog.definitions.module_identity(*dependency)?;
        visit(&dependency.definition())?;
    }
    Ok(())
}
