//! Scoped identity conversion; semantic impl correspondence remains the caller's policy.

use crate::update_source::{SourceBody, SourceEvidence, SourceFact, SourceRoot};
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
            parameters: self.parameters.remap_definitions(mapper)?,
            bounds: self.bounds.remap_definitions(mapper)?,
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
        self.parameters.visit_definition_ids(visit, cancel)?;
        self.bounds.visit_definition_ids(visit, cancel)?;
        self.facts.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for SourceRoot {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            declaration: mapper.reference(&self.declaration)?,
            kind: self.kind,
            receiver: self
                .receiver
                .as_ref()
                .map(|ty| ty.remap_definitions(mapper))
                .transpose()?,
            interface: self
                .interface
                .as_ref()
                .map(|ty| ty.remap_definitions(mapper))
                .transpose()?,
            members: map_sequence(&self.members, |id| mapper.reference(id))?,
        })
    }
}

impl DefinitionReferences for SourceRoot {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for member in &self.members {
            check_cancel(cancel)?;
            visit(member)?;
        }
        if let Some(receiver) = &self.receiver {
            receiver.visit_definition_ids(visit, cancel)?;
        }
        if let Some(interface) = &self.interface {
            interface.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for SourceEvidence {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            bodies: self.bodies.remap_definitions(mapper)?,
            roots: self.roots.remap_definitions(mapper)?,
            selections: self.selections.remap_definitions(mapper)?,
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
        self.roots.visit_definition_ids(visit, cancel)?;
        self.selections.visit_definition_ids(visit, cancel)
    }
}
