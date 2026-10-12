//! Reference coverage for attached registration facts; source arenas remain unchanged.

use crate::lower::LoweredModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::DefinitionId,
    },
};

impl DefinitionReferences for LoweredModule {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (id, value) in &self.registered_traits {
            check_cancel(cancel)?;
            visit(id)?;
            value.visit_definition_ids(visit, cancel)?;
        }
        for value in &self.registered_declarations {
            value.visit_definition_ids(visit, cancel)?;
        }
        for value in self.native_types.values() {
            value.visit_definition_ids(visit, cancel)?;
        }
        for value in self.native_functions.values() {
            value.visit_definition_ids(visit, cancel)?;
        }
        for value in self.native_trait_adapters.values() {
            value.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
