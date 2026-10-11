//! Explicit scoped identity traversal of semantic types and generic constraints.
use crate::ty::{Constraint, GenericBound, GenericParam, NominalTy};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, check_cancel, map_entries, map_sequence,
        },
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for NominalTy {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NominalTy {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
            associated_types: map_entries(
                self.associated_types.len(),
                self.associated_types.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
        })
    }
}

impl DefinitionReferences for NominalTy {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_types {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for GenericParam {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(GenericParam {
            owner: mapper.reference(&self.owner)?,
            position: self.position,
        })
    }
}

impl DefinitionReferences for GenericParam {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.owner)?;
        Ok(())
    }
}

impl RemapDefinitions for GenericBound {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(GenericBound {
            ty: self.ty.remap_definitions(mapper)?,
            constraints: map_sequence(&self.constraints, |value| {
                (value).remap_definitions(mapper)
            })?,
        })
    }
}

impl DefinitionReferences for GenericBound {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        for value0 in &self.constraints {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for Constraint {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Standard(field0) => Constraint::Standard(*(field0)),
            Self::Trait(field0) => Constraint::Trait((field0).remap_definitions(mapper)?),
        })
    }
}

impl DefinitionReferences for Constraint {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Standard(_) => {}
            Self::Trait(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}
