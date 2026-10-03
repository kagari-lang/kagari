//! Explicit traversal of the owning HIR records.
use crate::{AnalysisResult, AnalyzedModule, DeclaredAnalysis, PreparedAnalysis};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel},
        reference::DefinitionReference,
    },
};
use std::sync::Arc;

impl<I: DefinitionReference> DefinitionRecord<I> for AnalyzedModule<I> {
    type Rebind<J: DefinitionReference> = AnalyzedModule<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AnalyzedModule {
            aggregates: self.aggregates.map_identities(mapper)?,
            lowered: self.lowered.clone(),
            names: self.names.clone(),
            declarations: self.declarations.map_identities(mapper)?,
            typed: self.typed.map_identities(mapper)?,
            signatures: Arc::new((self.signatures.as_ref()).map_identities(mapper)?),
            imported_functions: self.imported_functions.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.aggregates.visit_definitions(visit, cancel)?;
        self.declarations.visit_definitions(visit, cancel)?;
        self.typed.visit_definitions(visit, cancel)?;
        (self.signatures.as_ref()).visit_definitions(visit, cancel)?;
        self.imported_functions.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for PreparedAnalysis<I> {
    type Rebind<J: DefinitionReference> = PreparedAnalysis<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(PreparedAnalysis {
            signatures_reused: self.signatures_reused,
            local_signature_diagnostics: self.local_signature_diagnostics,
            lowered: self.lowered.clone(),
            names: self.names.clone(),
            declarations: self.declarations.map_identities(mapper)?,
            signatures: Arc::new((self.signatures.as_ref()).map_identities(mapper)?),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.declarations.visit_definitions(visit, cancel)?;
        (self.signatures.as_ref()).visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for DeclaredAnalysis<I> {
    type Rebind<J: DefinitionReference> = DeclaredAnalysis<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(DeclaredAnalysis {
            lowered: self.lowered.clone(),
            names: self.names.clone(),
            declarations: self.declarations.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.declarations.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference, T: DefinitionRecord<I>> DefinitionRecord<I> for AnalysisResult<T> {
    type Rebind<J: DefinitionReference> = AnalysisResult<T::Rebind<J>>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        Ok(AnalysisResult {
            facts: self.facts.map_identities(mapper)?,
            diagnostics: self.diagnostics.clone(),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.facts.visit_definitions(visit, cancel)
    }
}
