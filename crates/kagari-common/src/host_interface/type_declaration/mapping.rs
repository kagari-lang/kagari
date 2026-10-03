//! Contextual identity traversal of the owning metadata records.
use crate::host_interface::type_declaration::{
    HostAssociatedTypeBinding, HostFieldDeclaration, HostMethodDeclaration,
    HostTraitImplementationDeclaration, HostTraitMethodBinding, HostTypeDeclaration,
};
use crate::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for HostFieldDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostFieldDeclaration<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostFieldDeclaration {
            id: mapper.reference(&self.id)?,
            name: self.name.clone(),
            ty: self.ty.map_identities(mapper)?,
            readable: self.readable,
            writable: self.writable,
            visibility: self.visibility,
            path_access: self.path_access,
            documentation: self.documentation.clone(),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.id)?;
        self.ty.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostMethodDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostMethodDeclaration<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostMethodDeclaration {
            id: mapper.reference(&self.id)?,
            name: self.name.clone(),
            receiver: self.receiver,
            params: map_sequence(&self.params, |value| (value).map_identities(mapper))?,
            return_type: self.return_type.map_identities(mapper)?,
            effects: self.effects,
            documentation: self.documentation.clone(),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.id)?;
        for value0 in &self.params {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.return_type.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostTraitMethodBinding<I> {
    type Rebind<J: DefinitionReference> = HostTraitMethodBinding<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostTraitMethodBinding {
            trait_method: mapper.reference(&self.trait_method)?,
            host_method: mapper.reference(&self.host_method)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.trait_method)?;
        check_cancel(cancel)?;
        visit(&self.host_method)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostAssociatedTypeBinding<I> {
    type Rebind<J: DefinitionReference> = HostAssociatedTypeBinding<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostAssociatedTypeBinding {
            declaration: mapper.reference(&self.declaration)?,
            ty: self.ty.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.ty.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostTraitImplementationDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostTraitImplementationDeclaration<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostTraitImplementationDeclaration {
            trait_id: mapper.reference(&self.trait_id)?,
            trait_arguments: map_sequence(&self.trait_arguments, |value| {
                (value).map_identities(mapper)
            })?,
            associated_types: map_sequence(&self.associated_types, |value| {
                (value).map_identities(mapper)
            })?,
            methods: map_sequence(&self.methods, |value| (value).map_identities(mapper))?,
            documentation: self.documentation.clone(),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.trait_id)?;
        for value0 in &self.trait_arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.associated_types {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostTypeDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostTypeDeclaration<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostTypeDeclaration {
            id: mapper.reference(&self.id)?,
            symbol: self.symbol.clone(),
            ownership: self.ownership,
            fields: map_sequence(&self.fields, |value| (value).map_identities(mapper))?,
            methods: map_sequence(&self.methods, |value| (value).map_identities(mapper))?,
            trait_implementations: map_sequence(&self.trait_implementations, |value| {
                (value).map_identities(mapper)
            })?,
            path_access: self.path_access,
            reflection: self.reflection,
            documentation: self.documentation.clone(),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.id)?;
        for value0 in &self.fields {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.trait_implementations {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
