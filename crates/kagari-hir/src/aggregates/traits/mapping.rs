//! Explicit traversal of the owning HIR records.
use crate::aggregates::traits::{
    AssociatedConstSignature, MethodDefault, MethodParameter, MethodSignature, TraitSignature,
};
use crate::identity_mapping::map_hash_entries;
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

impl<I: DefinitionReference> DefinitionRecord<I> for MethodParameter<I> {
    type Rebind<J: DefinitionReference> = MethodParameter<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(MethodParameter {
            name: self.name.clone(),
            writeability: self.writeability,
            ty: self.ty.map_identities(mapper)?,
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

impl<I: DefinitionReference> DefinitionRecord<I> for MethodSignature<I> {
    type Rebind<J: DefinitionReference> = MethodSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(MethodSignature {
            default: self
                .default
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            policy: self.policy,
            id: mapper.reference(&self.id)?,
            owner: mapper.reference(&self.owner)?,
            slot: self.slot,
            name: self.name.clone(),
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).map_identities(mapper)?,
                        map_sequence(value, |value| (value).map_identities(mapper))?,
                    ))
                }),
            )?,
            params: map_sequence(&self.params, |value| (value).map_identities(mapper))?,
            return_type: self.return_type.map_identities(mapper)?,
            declaration: self.declaration.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.default.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.id)?;
        check_cancel(cancel)?;
        visit(&self.owner)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definitions(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        for value0 in &self.params {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.return_type.visit_definitions(visit, cancel)?;
        self.declaration.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for MethodDefault<I> {
    type Rebind<J: DefinitionReference> = MethodDefault<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Script => MethodDefault::Script,
            Self::Native(field0) => MethodDefault::Native((field0).map_identities(mapper)?),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Script => {}
            Self::Native(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TraitSignature<I> {
    type Rebind<J: DefinitionReference> = TraitSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitSignature {
            associated_type_parameters: map_entries(
                self.associated_type_parameters.len(),
                self.associated_type_parameters.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
            associated_consts: map_entries(
                self.associated_consts.len(),
                self.associated_consts.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
            id: mapper.reference(&self.id)?,
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_hash_entries(
                self.bounds.len(),
                self.bounds.iter().map(|(key, value)| {
                    Ok((
                        (key).map_identities(mapper)?,
                        map_sequence(value, |value| (value).map_identities(mapper))?,
                    ))
                }),
            )?,
            supertraits: map_sequence(&self.supertraits, |value| (value).map_identities(mapper))?,
            methods: map_sequence(&self.methods, |value| (value).map_identities(mapper))?,
            declaration: self.declaration.map_identities(mapper)?,
            associated_types: map_entries(
                self.associated_types.len(),
                self.associated_types.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
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
        for (key0, value0) in &self.associated_type_parameters {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.associated_consts {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.id)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definitions(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        for value0 in &self.supertraits {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.declaration.visit_definitions(visit, cancel)?;
        for (key0, value0) in &self.associated_types {
            check_cancel(cancel)?;
            visit(key0)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for AssociatedConstSignature<I> {
    type Rebind<J: DefinitionReference> = AssociatedConstSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AssociatedConstSignature {
            declaration: self.declaration.map_identities(mapper)?,
            ty: self.ty.map_identities(mapper)?,
            initializer: self
                .initializer
                .as_ref()
                .map(|value| mapper.reference(value))
                .transpose()?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.declaration.visit_definitions(visit, cancel)?;
        self.ty.visit_definitions(visit, cancel)?;
        if let Some(value0) = self.initializer.as_ref() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        Ok(())
    }
}
