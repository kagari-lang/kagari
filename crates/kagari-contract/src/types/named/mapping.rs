use crate::types::named::{
    AliasContract, FunctionOrigin, ImplementationContract, NamedContract, NamedDeclaration,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for FunctionOrigin {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            placement: mapper.reference(&self.placement)?,
            authored: mapper.reference(&self.authored)?,
        })
    }
}

impl DefinitionReferences for FunctionOrigin {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.placement)?;
        visit(&self.authored)
    }
}

impl RemapDefinitions for NamedDeclaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            declaration: mapper.reference(&self.declaration)?,
            visibility: self.visibility,
            ambiguous_local_body: self.ambiguous_local_body.clone(),
            contract: self.contract.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for NamedDeclaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.contract.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for NamedContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Function(value) => Self::Function(value.remap_definitions(mapper)?),
            Self::Constant(value) => Self::Constant(value.remap_definitions(mapper)?),
            Self::Alias(value) => Self::Alias(AliasContract {
                name: value.name.clone(),
                generic_params: value.generic_params.remap_definitions(mapper)?,
                bounds: value.bounds.remap_definitions(mapper)?,
                target: value.target.remap_definitions(mapper)?,
            }),
            Self::Implementation(value) => Self::Implementation(ImplementationContract {
                bridge: value.bridge,
                generic_params: value.generic_params.remap_definitions(mapper)?,
                bounds: value.bounds.remap_definitions(mapper)?,
                receiver: value.receiver.remap_definitions(mapper)?,
                trait_type: value
                    .trait_type
                    .as_ref()
                    .map(|ty| ty.remap_definitions(mapper))
                    .transpose()?,
            }),
        })
    }
}

impl DefinitionReferences for NamedContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Function(value) => value.visit_definition_ids(visit, cancel),
            Self::Constant(value) => value.visit_definition_ids(visit, cancel),
            Self::Alias(value) => {
                value.generic_params.visit_definition_ids(visit, cancel)?;
                value.bounds.visit_definition_ids(visit, cancel)?;
                value.target.visit_definition_ids(visit, cancel)
            }
            Self::Implementation(value) => {
                value.generic_params.visit_definition_ids(visit, cancel)?;
                value.bounds.visit_definition_ids(visit, cancel)?;
                value.receiver.visit_definition_ids(visit, cancel)?;
                if let Some(ty) = &value.trait_type {
                    ty.visit_definition_ids(visit, cancel)?;
                }
                Ok(())
            }
        }
    }
}
