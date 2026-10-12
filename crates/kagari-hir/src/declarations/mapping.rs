//! Explicit traversal of the owning HIR records.

use crate::declarations::{BindingId, Declaration, DeclarationId, Declarations};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for DeclarationId {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Definition(field0) => DeclarationId::Definition(mapper.reference(field0)?),
            Self::GenericParameter { owner, position } => DeclarationId::GenericParameter {
                owner: mapper.reference(owner)?,
                position: *(position),
            },
            Self::Binding(field0) => DeclarationId::Binding(BindingId {
                body: mapper.reference(&field0.body)?,
                ..field0.clone()
            }),
        })
    }
}

impl DefinitionReferences for DeclarationId {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Definition(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::GenericParameter { owner, position: _ } => {
                check_cancel(cancel)?;
                visit(owner)?;
            }
            Self::Binding(binding) => visit(&binding.body)?,
        }
        Ok(())
    }
}

impl RemapDefinitions for Declaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Declaration {
            id: self.id.remap_definitions(mapper)?,
            name: self.name.clone(),
            location: self.location,
        })
    }
}

impl DefinitionReferences for Declaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.id.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl DefinitionReferences for Declarations {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for id in self.language_items.values() {
            check_cancel(cancel)?;
            visit(id)?;
        }
        self.hosts.visit_definition_ids(visit, cancel)?;
        self.imported_types.visit_definition_ids(visit, cancel)?;
        for value0 in self.targets.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for key0 in self.identities.keys() {
            (key0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.impl_identities.values() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        for value0 in self.native_types.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }

        Ok(())
    }
}
