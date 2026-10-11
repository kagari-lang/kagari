//! Contextual identity traversal of the owning metadata records.
use crate::host_interface::type_declaration::{
    HostAssociatedTypeBinding, HostFieldDeclaration, HostMethodDeclaration,
    HostTraitImplementationDeclaration, HostTraitMethodBinding, HostTypeDeclaration,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for HostFieldDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostFieldDeclaration {
            id: mapper.reference(&self.id)?,
            name: self.name.clone(),
            ty: self.ty.remap_definitions(mapper)?,
            readable: self.readable,
            writable: self.writable,
            visibility: self.visibility,
            path_access: self.path_access,
            documentation: self.documentation.clone(),
        })
    }
}

impl DefinitionReferences for HostFieldDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.id)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for HostMethodDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostMethodDeclaration {
            id: mapper.reference(&self.id)?,
            name: self.name.clone(),
            receiver: self.receiver,
            params: map_sequence(&self.params, |value| (value).remap_definitions(mapper))?,
            return_type: self.return_type.remap_definitions(mapper)?,
            effects: self.effects,
            documentation: self.documentation.clone(),
        })
    }
}

impl DefinitionReferences for HostMethodDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.id)?;
        for value0 in &self.params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.return_type.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for HostTraitMethodBinding {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostTraitMethodBinding {
            trait_method: mapper.reference(&self.trait_method)?,
            host_method: mapper.reference(&self.host_method)?,
        })
    }
}

impl DefinitionReferences for HostTraitMethodBinding {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.trait_method)?;
        check_cancel(cancel)?;
        visit(&self.host_method)?;
        Ok(())
    }
}

impl RemapDefinitions for HostAssociatedTypeBinding {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostAssociatedTypeBinding {
            declaration: mapper.reference(&self.declaration)?,
            ty: self.ty.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for HostAssociatedTypeBinding {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for HostTraitImplementationDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostTraitImplementationDeclaration {
            trait_id: mapper.reference(&self.trait_id)?,
            trait_arguments: map_sequence(&self.trait_arguments, |value| {
                (value).remap_definitions(mapper)
            })?,
            associated_types: map_sequence(&self.associated_types, |value| {
                (value).remap_definitions(mapper)
            })?,
            methods: map_sequence(&self.methods, |value| (value).remap_definitions(mapper))?,
            documentation: self.documentation.clone(),
        })
    }
}

impl DefinitionReferences for HostTraitImplementationDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.trait_id)?;
        for value0 in &self.trait_arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.associated_types {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.methods {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for HostTypeDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostTypeDeclaration {
            id: mapper.reference(&self.id)?,
            symbol: self.symbol.clone(),
            ownership: self.ownership,
            fields: map_sequence(&self.fields, |value| (value).remap_definitions(mapper))?,
            methods: map_sequence(&self.methods, |value| (value).remap_definitions(mapper))?,
            trait_implementations: map_sequence(&self.trait_implementations, |value| {
                (value).remap_definitions(mapper)
            })?,
            path_access: self.path_access,
            reflection: self.reflection,
            documentation: self.documentation.clone(),
        })
    }
}

impl DefinitionReferences for HostTypeDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.id)?;
        for value0 in &self.fields {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.methods {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.trait_implementations {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
