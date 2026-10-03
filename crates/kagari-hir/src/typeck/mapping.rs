//! Explicit traversal of the owning HIR records.
use crate::identity_mapping::map_hash_entries;
use crate::typeck::{
    FunctionImplementation, ModuleSignatures, TypedFunction, TypedModule, TypedParameter,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for ModuleSignatures<I> {
    type Rebind<J: DefinitionReference> = ModuleSignatures<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ModuleSignatures {
            type_bounds: map_hash_entries(
                self.type_bounds.len(),
                self.type_bounds.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        map_hash_entries(
                            (value).len(),
                            (value).iter().map(|(key, value)| {
                                Ok((
                                    (key).map_identities(mapper)?,
                                    map_sequence(value, |value| (value).map_identities(mapper))?,
                                ))
                            }),
                        )?,
                    ))
                }),
            )?,
            functions: map_sequence(self.functions.as_slice(), |value| {
                (value).map_identities(mapper)
            })?
            .into_iter()
            .collect(),
            type_table: self.type_table.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (key0, value0) in &self.type_bounds {
            check_cancel(cancel)?;
            visit(key0)?;
            for (key1, value1) in value0 {
                (key1).visit_definitions(visit, cancel)?;
                for value2 in value1 {
                    (value2).visit_definitions(visit, cancel)?;
                }
            }
        }
        for value0 in &self.functions {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.type_table.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TypedModule<I> {
    type Rebind<J: DefinitionReference> = TypedModule<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypedModule {
            checked_bodies: self.checked_bodies,
            reused_bodies: self.reused_bodies,
            functions: map_sequence(self.functions.as_slice(), |value| {
                (value).map_identities(mapper)
            })?
            .into_iter()
            .collect(),
            consts: map_hash_entries(
                self.consts.len(),
                self.consts
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            const_values: self.const_values.clone(),
            type_table: self.type_table.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.functions {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.consts.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.type_table.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TypedFunction<I> {
    type Rebind<J: DefinitionReference> = TypedFunction<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypedFunction {
            implementation: self.implementation.map_identities(mapper)?,
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
            id: self.id,
            name: self.name.clone(),
            params: map_sequence(self.params.as_slice(), |value| {
                (value).map_identities(mapper)
            })?
            .into_iter()
            .collect(),
            return_type: self.return_type.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.implementation.visit_definitions(visit, cancel)?;
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
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for FunctionImplementation<I> {
    type Rebind<J: DefinitionReference> = FunctionImplementation<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Script => FunctionImplementation::Script,
            Self::Native(field0) => {
                FunctionImplementation::Native((field0).map_identities(mapper)?)
            }
            Self::Required => FunctionImplementation::Required,
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
            Self::Required => {}
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TypedParameter<I> {
    type Rebind<J: DefinitionReference> = TypedParameter<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TypedParameter {
            id: self.id,
            writeability: self.writeability,
            name: self.name.clone(),
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
