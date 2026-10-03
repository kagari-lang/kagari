//! Explicit traversal of the owning HIR records.
use crate::identity_mapping::map_hash_entries;
use crate::types::shape;
use crate::types::{
    AssociatedTypeFamily, AssociatedTypeParameters, GenericParameterType, NominalType, TypeId,
    TypeSubstitution,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_entries,
            map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for TypeSubstitution<I> {
    type Rebind<J: DefinitionReference> = TypeSubstitution<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypeSubstitution {
            parameters: map_hash_entries(
                self.parameters.len(),
                self.parameters.iter().map(|(key, value)| {
                    Ok((
                        (key).map_identities(mapper)?,
                        (value).map_identities(mapper)?,
                    ))
                }),
            )?,
            receivers: map_hash_entries(
                self.receivers.len(),
                self.receivers.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (key0, value0) in &self.parameters {
            (key0).visit_definitions(visit, cancel)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.receivers {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for GenericParameterType<I> {
    type Rebind<J: DefinitionReference> = GenericParameterType<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(GenericParameterType {
            owner: mapper.reference(&self.owner)?,
            position: self.position,
            name: self.name.clone(),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        check_cancel(cancel)?;
        visit(&self.owner)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedTypeParameters<I> {
    type Rebind<J: DefinitionReference> = AssociatedTypeParameters<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeParameters {
            parameters: map_sequence(&self.parameters, |value| (value).map_identities(mapper))?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).map_identities(mapper)?,
                        map_sequence(value, |value| (value).map_identities(mapper))?,
                    ))
                }),
            )?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.parameters {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definitions(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedTypeFamily<I> {
    type Rebind<J: DefinitionReference> = AssociatedTypeFamily<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedTypeFamily {
            inputs: self.inputs.map_identities(mapper)?,
            value: self.value.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.inputs.visit_definitions(visit, cancel)?;
        self.value.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for NominalType<I> {
    type Rebind<J: DefinitionReference> = NominalType<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NominalType {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
            associated_types: map_entries(
                self.associated_types.len(),
                self.associated_types.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
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
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_types {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TypeId<I> {
    type Rebind<J: DefinitionReference> = TypeId<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        shape::validate(self, mapper.cancellation())?;
        Ok(match self {
            Self::Inference(field0) => TypeId::Inference(*(field0)),
            Self::Unknown => TypeId::Unknown,
            Self::Error => TypeId::Error,
            Self::Builtin(field0) => TypeId::Builtin(*(field0)),
            Self::Tuple(field0) => TypeId::Tuple(map_sequence(field0, |value| {
                (value).map_identities(mapper)
            })?),
            Self::Function { params, result } => TypeId::Function {
                params: map_sequence(params, |value| (value).map_identities(mapper))?,
                result: Box::new(((result).as_ref()).map_identities(mapper)?),
            },
            Self::Iter(field0) => {
                TypeId::Iter(Box::new(((field0).as_ref()).map_identities(mapper)?))
            }
            Self::Range(field0, field1) => TypeId::Range(
                Box::new(((field0).as_ref()).map_identities(mapper)?),
                *(field1),
            ),
            Self::Array(field0, field1) => TypeId::Array(
                Box::new(((field0).as_ref()).map_identities(mapper)?),
                *(field1),
            ),
            Self::Map { key, value, access } => TypeId::Map {
                key: Box::new(((key).as_ref()).map_identities(mapper)?),
                value: Box::new(((value).as_ref()).map_identities(mapper)?),
                access: *(access),
            },
            Self::Set(field0, field1) => TypeId::Set(
                Box::new(((field0).as_ref()).map_identities(mapper)?),
                *(field1),
            ),
            Self::NativeObject(field0) => TypeId::NativeObject((field0).map_identities(mapper)?),
            Self::Struct(field0) => TypeId::Struct((field0).map_identities(mapper)?),
            Self::Enum(field0) => TypeId::Enum((field0).map_identities(mapper)?),
            Self::Trait(field0) => TypeId::Trait((field0).map_identities(mapper)?),
            Self::Host(field0) => TypeId::Host(mapper.reference(field0)?),
            Self::Generic(field0) => TypeId::Generic((field0).map_identities(mapper)?),
            Self::Projection {
                arguments,
                receiver,
                interface,
                member,
            } => TypeId::Projection {
                arguments: map_sequence(arguments, |value| (value).map_identities(mapper))?,
                receiver: Box::new(((receiver).as_ref()).map_identities(mapper)?),
                interface: Box::new(((interface).as_ref()).map_identities(mapper)?),
                member: mapper.reference(member)?,
            },
            Self::SelfType(field0) => TypeId::SelfType(mapper.reference(field0)?),
            Self::StandardEnum { kind, args } => TypeId::StandardEnum {
                kind: *(kind),
                args: map_sequence(args, |value| (value).map_identities(mapper))?,
            },
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        shape::validate(self, cancel)?;
        match self {
            Self::Inference(_) => {}
            Self::Unknown => {}
            Self::Error => {}
            Self::Builtin(_) => {}
            Self::Tuple(field0) => {
                for value0 in field0 {
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
            Self::Function { params, result } => {
                for value0 in params {
                    (value0).visit_definitions(visit, cancel)?;
                }
                ((result).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Iter(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Range(field0, _) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Array(field0, _) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Map {
                key,
                value,
                access: _,
            } => {
                ((key).as_ref()).visit_definitions(visit, cancel)?;
                ((value).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::Set(field0, _) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
            Self::NativeObject(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Struct(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Enum(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Trait(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Host(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::Generic(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Projection {
                arguments,
                receiver,
                interface,
                member,
            } => {
                for value0 in arguments {
                    (value0).visit_definitions(visit, cancel)?;
                }
                ((receiver).as_ref()).visit_definitions(visit, cancel)?;
                ((interface).as_ref()).visit_definitions(visit, cancel)?;
                check_cancel(cancel)?;
                visit(member)?;
            }
            Self::SelfType(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::StandardEnum { kind: _, args } => {
                for value0 in args {
                    (value0).visit_definitions(visit, cancel)?;
                }
            }
        }
        Ok(())
    }
}
