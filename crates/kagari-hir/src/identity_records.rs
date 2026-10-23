//! Explicit traversal of the owning HIR records.

use crate::{AnalysisResult, AnalyzedModule, DeclaredAnalysis, PreparedAnalysis};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::DefinitionId,
    },
};

impl DefinitionReferences for AnalyzedModule {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.lowered.visit_definition_ids(visit, cancel)?;
        self.names.visit_definition_ids(visit, cancel)?;
        self.aggregates.visit_definition_ids(visit, cancel)?;
        self.declarations.visit_definition_ids(visit, cancel)?;
        self.typed.visit_definition_ids(visit, cancel)?;
        (self.signatures.as_ref()).visit_definition_ids(visit, cancel)?;
        self.imported_functions
            .visit_definition_ids(visit, cancel)?;
        self.imported_statics.visit_definition_ids(visit, cancel)?;
        self.imported_constants
            .visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl DefinitionReferences for PreparedAnalysis {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.lowered.visit_definition_ids(visit, cancel)?;
        self.names.visit_definition_ids(visit, cancel)?;
        self.declarations.visit_definition_ids(visit, cancel)?;
        (self.signatures.as_ref()).visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl DefinitionReferences for DeclaredAnalysis {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.lowered.visit_definition_ids(visit, cancel)?;
        self.names.visit_definition_ids(visit, cancel)?;
        self.declarations.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl<T: DefinitionReferences> DefinitionReferences for AnalysisResult<T> {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.facts.visit_definition_ids(visit, cancel)
    }
}
