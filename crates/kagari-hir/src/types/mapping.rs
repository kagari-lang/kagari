//! Explicit traversal of the owning HIR records.

use crate::{
    identity_mapping::map_hash_entries,
    types::{
        AssociatedTypeFamily, AssociatedTypeParameters, GenericParameterType, NominalType, TypeId,
        TypeSubstitution, shape,
    },
};
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

impl RemapDefinitions for TypeSubstitution {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypeSubstitution {
            parameters: map_hash_entries(
                self.parameters.len(),
                self.parameters.iter().map(|(key, value)| {
                    Ok((
                        (key).remap_definitions(mapper)?,
                        (value).remap_definitions(mapper)?,
                    ))
                }),
            )?,
            receivers: map_hash_entries(
                self.receivers.len(),
                self.receivers.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).remap_definitions(mapper)?))
                }),
            )?,
        })
    }
}

impl DefinitionReferences for TypeSubstitution {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (key0, value0) in &self.parameters {
            (key0).visit_definition_ids(visit, cancel)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.receivers {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for GenericParameterType {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(GenericParameterType {
            owner: mapper.reference(&self.owner)?,
            position: self.position,
            name: self.name.clone(),
        })
    }
}

impl DefinitionReferences for GenericParameterType {
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

impl RemapDefinitions for AssociatedTypeParameters {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeParameters {
            parameters: map_sequence(&self.parameters, |value| (value).remap_definitions(mapper))?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).remap_definitions(mapper)?,
                        map_sequence(value, |value| (value).remap_definitions(mapper))?,
                    ))
                }),
            )?,
        })
    }
}

impl DefinitionReferences for AssociatedTypeParameters {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.parameters {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definition_ids(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for AssociatedTypeFamily {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeFamily {
            inputs: self.inputs.remap_definitions(mapper)?,
            value: self.value.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for AssociatedTypeFamily {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.inputs.visit_definition_ids(visit, cancel)?;
        self.value.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for NominalType {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NominalType {
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

impl DefinitionReferences for NominalType {
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

impl RemapDefinitions for TypeId {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        shape::validate(self, mapper.cancellation())?;
        Ok(match self {
            Self::Reference { target, access } => TypeId::Reference {
                target: Box::new(target.remap_definitions(mapper)?),
                access: *access,
            },
            Self::Inference(field0) => TypeId::Inference(*(field0)),
            Self::Unknown => TypeId::Unknown,
            Self::Error => TypeId::Error,
            Self::Builtin(field0) => TypeId::Builtin(*(field0)),
            Self::Tuple(field0) => TypeId::Tuple(map_sequence(field0, |value| {
                (value).remap_definitions(mapper)
            })?),
            Self::Function { params, result } => TypeId::Function {
                params: map_sequence(params, |value| (value).remap_definitions(mapper))?,
                result: Box::new(((result).as_ref()).remap_definitions(mapper)?),
            },
            Self::Iter(field0) => {
                TypeId::Iter(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
            Self::Range(field0, field1) => TypeId::Range(
                Box::new(((field0).as_ref()).remap_definitions(mapper)?),
                *(field1),
            ),
            Self::Array(field0) => {
                TypeId::Array(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
            Self::Map { key, value, access } => TypeId::Map {
                key: Box::new(((key).as_ref()).remap_definitions(mapper)?),
                value: Box::new(((value).as_ref()).remap_definitions(mapper)?),
                access: *(access),
            },
            Self::Set(field0, field1) => TypeId::Set(
                Box::new(((field0).as_ref()).remap_definitions(mapper)?),
                *(field1),
            ),
            Self::NativeObject(field0) => TypeId::NativeObject((field0).remap_definitions(mapper)?),
            Self::Struct(field0) => TypeId::Struct((field0).remap_definitions(mapper)?),
            Self::Enum(field0) => TypeId::Enum((field0).remap_definitions(mapper)?),
            Self::Trait(field0) => TypeId::Trait((field0).remap_definitions(mapper)?),
            Self::Host(field0) => TypeId::Host(mapper.reference(field0)?),
            Self::Generic(field0) => TypeId::Generic((field0).remap_definitions(mapper)?),
            Self::Projection {
                arguments,
                receiver,
                interface,
                member,
            } => TypeId::Projection {
                arguments: map_sequence(arguments, |value| (value).remap_definitions(mapper))?,
                receiver: Box::new(((receiver).as_ref()).remap_definitions(mapper)?),
                interface: Box::new(((interface).as_ref()).remap_definitions(mapper)?),
                member: mapper.reference(member)?,
            },
            Self::SelfType(field0) => TypeId::SelfType(mapper.reference(field0)?),
        })
    }
}

impl DefinitionReferences for TypeId {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        shape::validate(self, cancel)?;
        match self {
            Self::Reference { target, .. } => target.visit_definition_ids(visit, cancel)?,
            Self::Inference(_) => {}
            Self::Unknown => {}
            Self::Error => {}
            Self::Builtin(_) => {}
            Self::Tuple(field0) => {
                for value0 in field0 {
                    (value0).visit_definition_ids(visit, cancel)?;
                }
            }
            Self::Function { params, result } => {
                for value0 in params {
                    (value0).visit_definition_ids(visit, cancel)?;
                }
                ((result).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Iter(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Range(field0, _) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Array(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Map {
                key,
                value,
                access: _,
            } => {
                ((key).as_ref()).visit_definition_ids(visit, cancel)?;
                ((value).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::Set(field0, _) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
            Self::NativeObject(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Struct(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Enum(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Trait(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Host(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::Generic(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Projection {
                arguments,
                receiver,
                interface,
                member,
            } => {
                for value0 in arguments {
                    (value0).visit_definition_ids(visit, cancel)?;
                }
                ((receiver).as_ref()).visit_definition_ids(visit, cancel)?;
                ((interface).as_ref()).visit_definition_ids(visit, cancel)?;
                check_cancel(cancel)?;
                visit(member)?;
            }
            Self::SelfType(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
        }
        Ok(())
    }
}
