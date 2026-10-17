//! Resolution inputs retain source-unit handles, not canonical definition IDs.
//! Namespace lookup validates those handles against the retained catalog.

use crate::resolver::resolved::{DeclarationNames, ResolvedNames};
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
        _visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(())
    }
}

impl DefinitionReferences for ResolvedNames {
    fn visit_definition_ids(
        &self,
        _visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(())
    }
}
