//! Explicit traversal of the owning HIR records.

use crate::aggregates::traits::{
    AssociatedConstSignature, MethodDefault, MethodParameter, MethodSignature, TraitSignature,
};
use crate::identity_mapping::map_hash_entries;
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

impl RemapDefinitions for MethodParameter {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(MethodParameter {
            name: self.name.clone(),
            writeability: self.writeability,
            ty: self.ty.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for MethodParameter {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for MethodSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(MethodSignature {
            default: self
                .default
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            policy: self.policy,
            id: mapper.reference(&self.id)?,
            owner: mapper.reference(&self.owner)?,
            slot: self.slot,
            name: self.name.clone(),
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).remap_definitions(mapper)?,
                        map_sequence(value, |value| (value).remap_definitions(mapper))?,
                    ))
                }),
            )?,
            params: map_sequence(&self.params, |value| (value).remap_definitions(mapper))?,
            return_type: self.return_type.remap_definitions(mapper)?,
            declaration: self.declaration.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for MethodSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.default.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.id)?;
        check_cancel(cancel)?;
        visit(&self.owner)?;
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definition_ids(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        for value0 in &self.params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.return_type.visit_definition_ids(visit, cancel)?;
        self.declaration.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for MethodDefault {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Script => MethodDefault::Script,
            Self::Native(field0) => MethodDefault::Native((field0).remap_definitions(mapper)?),
        })
    }
}

impl DefinitionReferences for MethodDefault {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Script => {}
            Self::Native(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for TraitSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitSignature {
            storage_access: self.storage_access,
            conversion_adapter: self
                .conversion_adapter
                .as_ref()
                .map(|adapter| adapter.remap_definitions(mapper))
                .transpose()?,
            associated_type_parameters: map_entries(
                self.associated_type_parameters.len(),
                self.associated_type_parameters.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
            associated_consts: map_entries(
                self.associated_consts.len(),
                self.associated_consts.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
            id: mapper.reference(&self.id)?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).remap_definitions(mapper)?,
                        map_sequence(value, |value| (value).remap_definitions(mapper))?,
                    ))
                }),
            )?,
            supertraits: map_sequence(&self.supertraits, |value| {
                (value).remap_definitions(mapper)
            })?,
            methods: map_sequence(&self.methods, |value| (value).remap_definitions(mapper))?,
            declaration: self.declaration.remap_definitions(mapper)?,
            associated_types: map_entries(
                self.associated_types.len(),
                self.associated_types.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        map_sequence(value, |value| (value).remap_definitions(mapper))?,
                    ))
                }),
            )?,
        })
    }
}

impl DefinitionReferences for TraitSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(adapter) = &self.conversion_adapter {
            adapter.visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_type_parameters {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_consts {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.id)?;
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definition_ids(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        for value0 in &self.supertraits {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.methods {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.declaration.visit_definition_ids(visit, cancel)?;
        for (key0, value0) in &self.associated_types {
            check_cancel(cancel)?;
            visit(key0)?;
            for value1 in value0 {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for AssociatedConstSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedConstSignature {
            declaration: self.declaration.remap_definitions(mapper)?,
            ty: self.ty.remap_definitions(mapper)?,
            initializer: self
                .initializer
                .as_ref()
                .map(|value| mapper.reference(value))
                .transpose()?,
        })
    }
}

impl DefinitionReferences for AssociatedConstSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.declaration.visit_definition_ids(visit, cancel)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        if let Some(value0) = self.initializer.as_ref() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        Ok(())
    }
}
