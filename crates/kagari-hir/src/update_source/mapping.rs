//! Scoped identity conversion; semantic correspondence remains the caller's policy.

use crate::update_source::{SourceBody, SourceEvidence, SourceFact, SourceInitializer};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for SourceFact {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            site: self.site,
            kind: self.kind,
            related: self.related.clone(),
            targets: map_sequence(&self.targets, |id| mapper.reference(id))?,
            types: self.types.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for SourceFact {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for id in &self.targets {
            check_cancel(cancel)?;
            visit(id)?;
        }
        self.types.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for SourceBody {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            declaration: mapper.reference(&self.declaration)?,
            kind: self.kind,
            tokens: self.tokens.clone(),
            initializer: self.initializer,
            facts: self.facts.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for SourceBody {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.facts.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for SourceInitializer {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            declaration: mapper.reference(&self.declaration)?,
        })
    }
}

impl DefinitionReferences for SourceInitializer {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)
    }
}

impl RemapDefinitions for SourceEvidence {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            bodies: self.bodies.remap_definitions(mapper)?,
            initializers: self.initializers.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for SourceEvidence {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.bodies.visit_definition_ids(visit, cancel)?;
        self.initializers.visit_definition_ids(visit, cancel)
    }
}
