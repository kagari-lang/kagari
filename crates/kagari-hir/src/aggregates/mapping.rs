//! Explicit traversal of the owning HIR records.
use crate::aggregates::{
    AggregateCatalog, EnumSignature, FieldSignature, InherentMethodSignature, NativeTypeSignature,
    StructSignature, VariantSignature,
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
use std::sync::Arc;

impl<I: DefinitionReference> DefinitionRecord<I> for FieldSignature<I> {
    type Rebind<J: DefinitionReference> = FieldSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FieldSignature {
            id: mapper.reference(&self.id)?,
            owner: mapper.reference(&self.owner)?,
            slot: self.slot,
            name: self.name.clone(),
            visibility: self.visibility,
            writeability: self.writeability,
            ty: self.ty.map_identities(mapper)?,
            declaration: self.declaration.map_identities(mapper)?,
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
        check_cancel(cancel)?;
        visit(&self.owner)?;
        self.ty.visit_definitions(visit, cancel)?;
        self.declaration.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for InherentMethodSignature<I> {
    type Rebind<J: DefinitionReference> = InherentMethodSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InherentMethodSignature {
            id: self.id.clone(),
            declaration: mapper.reference(&self.declaration)?,
            site: self.site.map_identities(mapper)?,
            owner: self.owner.map_identities(mapper)?,
            visibility: self.visibility,
            function: self.function.map_identities(mapper)?,
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
        self.site.visit_definitions(visit, cancel)?;
        self.owner.visit_definitions(visit, cancel)?;
        self.function.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for StructSignature<I> {
    type Rebind<J: DefinitionReference> = StructSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(StructSignature {
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
            declaration: self.declaration.map_identities(mapper)?,
            fields: map_sequence(&self.fields, |value| (value).map_identities(mapper))?,
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
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definitions(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        self.declaration.visit_definitions(visit, cancel)?;
        for value0 in &self.fields {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for NativeTypeSignature<I> {
    type Rebind<J: DefinitionReference> = NativeTypeSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeTypeSignature {
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
            declaration: self.declaration.map_identities(mapper)?,
            representation: self.representation.map_identities(mapper)?,
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
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definitions(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        self.declaration.visit_definitions(visit, cancel)?;
        self.representation.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for VariantSignature<I> {
    type Rebind<J: DefinitionReference> = VariantSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(VariantSignature {
            reports_failure: self.reports_failure,
            id: mapper.reference(&self.id)?,
            owner: mapper.reference(&self.owner)?,
            slot: self.slot,
            name: self.name.clone(),
            payload: map_sequence(&self.payload, |value| (value).map_identities(mapper))?,
            declaration: self.declaration.map_identities(mapper)?,
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
        check_cancel(cancel)?;
        visit(&self.owner)?;
        for value0 in &self.payload {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.declaration.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for EnumSignature<I> {
    type Rebind<J: DefinitionReference> = EnumSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(EnumSignature {
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
            declaration: self.declaration.map_identities(mapper)?,
            variants: map_sequence(&self.variants, |value| (value).map_identities(mapper))?,
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

        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definitions(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        self.declaration.visit_definitions(visit, cancel)?;
        for value0 in &self.variants {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for AggregateCatalog<I> {
    type Rebind<J: DefinitionReference> = AggregateCatalog<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(AggregateCatalog {
            language_items: self
                .language_items
                .iter()
                .map(|(role, id)| Ok((*role, mapper.reference(id)?)))
                .collect::<Result<_, DefinitionMappingError>>()?,
            implementation_constants: map_entries(
                self.implementation_constants.len(),
                self.implementation_constants.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        map_entries(
                            (value).len(),
                            (value).iter().map(|(key, value)| {
                                Ok((mapper.reference(key)?, mapper.reference(value)?))
                            }),
                        )?,
                    ))
                }),
            )?,
            host_implementations: map_sequence(&self.host_implementations, |value| {
                Ok((
                    (value).0.map_identities(mapper)?,
                    (value).1.map_identities(mapper)?,
                ))
            })?,
            traits: map_entries(
                self.traits.len(),
                self.traits.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).map_identities(mapper)?),
                    ))
                }),
            )?,
            implementations: map_entries(
                self.implementations.len(),
                self.implementations.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).map_identities(mapper)?),
                    ))
                }),
            )?,
            methods: map_entries(
                self.methods.len(),
                self.methods.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        (mapper.reference(&(value).0)?, (value).1),
                    ))
                }),
            )?,
            inherent_methods: map_entries(
                self.inherent_methods.len(),
                self.inherent_methods.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).map_identities(mapper)?),
                    ))
                }),
            )?,
            native_types: map_entries(
                self.native_types.len(),
                self.native_types.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).map_identities(mapper)?),
                    ))
                }),
            )?,
            structures: map_entries(
                self.structures.len(),
                self.structures.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).map_identities(mapper)?),
                    ))
                }),
            )?,
            fields: map_entries(
                self.fields.len(),
                self.fields.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        (mapper.reference(&(value).0)?, (value).1),
                    ))
                }),
            )?,
            enumerations: map_entries(
                self.enumerations.len(),
                self.enumerations.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).map_identities(mapper)?),
                    ))
                }),
            )?,
            variants: map_entries(
                self.variants.len(),
                self.variants.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        (mapper.reference(&(value).0)?, (value).1),
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
        for (key0, value0) in &self.implementation_constants {
            check_cancel(cancel)?;
            visit(key0)?;
            for (key1, value1) in value0 {
                check_cancel(cancel)?;
                visit(key1)?;
                check_cancel(cancel)?;
                visit(value1)?;
            }
        }
        for id in self.language_items.values() {
            check_cancel(cancel)?;
            visit(id)?;
        }
        for value0 in &self.host_implementations {
            (value0).0.visit_definitions(visit, cancel)?;
            (value0).1.visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.traits {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.implementations {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.methods {
            check_cancel(cancel)?;
            visit(key0)?;
            check_cancel(cancel)?;
            visit(&(value0).0)?;
        }
        for (key0, value0) in &self.inherent_methods {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.native_types {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.structures {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.fields {
            check_cancel(cancel)?;
            visit(key0)?;
            check_cancel(cancel)?;
            visit(&(value0).0)?;
        }
        for (key0, value0) in &self.enumerations {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.variants {
            check_cancel(cancel)?;
            visit(key0)?;
            check_cancel(cancel)?;
            visit(&(value0).0)?;
        }
        Ok(())
    }
}
