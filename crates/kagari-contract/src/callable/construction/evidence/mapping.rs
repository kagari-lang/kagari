//! Identity remapping of construction capabilities and selected dependencies.
use crate::callable::construction::evidence::{
    ConstructionRequirement, ConstructionSelection, ConstructionWitness,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for ConstructionRequirement {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            receiver: self.receiver.remap_definitions(mapper)?,
            interface: self.interface.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for ConstructionRequirement {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.receiver.visit_definition_ids(visit, cancel)?;
        self.interface.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for ConstructionWitness {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        self.check_shape(mapper.cancellation())?;
        Ok(Self {
            requirement: self.requirement.remap_definitions(mapper)?,
            selection: match &self.selection {
                ConstructionSelection::Declared {
                    implementation,
                    operations,
                    constructions,
                } => ConstructionSelection::Declared {
                    implementation: implementation.remap_definitions(mapper)?,
                    operations: map_sequence(operations, |child| child.remap_definitions(mapper))?,
                    constructions: map_sequence(constructions, |child| {
                        child.remap_definitions(mapper)
                    })?,
                },
                ConstructionSelection::Interface => ConstructionSelection::Interface,
                ConstructionSelection::Forward => ConstructionSelection::Forward,
            },
        })
    }
}

impl DefinitionReferences for ConstructionWitness {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        let mut pending = vec![self];
        while let Some(witness) = pending.pop() {
            check_cancel(cancel)?;
            witness.requirement.visit_definition_ids(visit, cancel)?;
            if let ConstructionSelection::Declared {
                implementation,
                operations,
                constructions,
            } = &witness.selection
            {
                implementation.visit_definition_ids(visit, cancel)?;
                for operation in operations {
                    operation.visit_definition_ids(visit, cancel)?;
                }
                pending.extend(constructions.iter().rev());
            }
        }
        Ok(())
    }
}
