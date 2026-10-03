//! Contextual identity traversal of the owning metadata records.
use crate::host_interface::{HostFunctionDeclaration, HostInterface, HostParameter};
use crate::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for HostParameter<I> {
    type Rebind<J: DefinitionReference> = HostParameter<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostParameter {
            name: self.name.clone(),
            ty: self.ty.map_identities(mapper)?,
            passing: self.passing,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for HostFunctionDeclaration<I> {
    type Rebind<J: DefinitionReference> = HostFunctionDeclaration<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostFunctionDeclaration {
            id: mapper.reference(&self.id)?,
            symbol: self.symbol.clone(),
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

impl<I: DefinitionReference> DefinitionRecord<I> for HostInterface<I> {
    type Rebind<J: DefinitionReference> = HostInterface<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(HostInterface {
            paths: map_sequence(&self.paths, |value| (value).map_identities(mapper))?,
            types: map_sequence(&self.types, |value| (value).map_identities(mapper))?,
            functions: map_sequence(&self.functions, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.paths {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.types {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.functions {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
