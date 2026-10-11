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
            DefinitionMapper, DefinitionMappingError, check_cancel, map_entries, map_sequence,
        },
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use std::sync::Arc;

impl RemapDefinitions for FieldSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(FieldSignature {
            id: mapper.reference(&self.id)?,
            owner: mapper.reference(&self.owner)?,
            slot: self.slot,
            name: self.name.clone(),
            visibility: self.visibility,
            writeability: self.writeability,
            ty: self.ty.remap_definitions(mapper)?,
            declaration: self.declaration.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for FieldSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.id)?;
        check_cancel(cancel)?;
        visit(&self.owner)?;
        self.ty.visit_definition_ids(visit, cancel)?;
        self.declaration.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for InherentMethodSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InherentMethodSignature {
            id: self.id.clone(),
            declaration: mapper.reference(&self.declaration)?,
            site: self.site.remap_definitions(mapper)?,
            owner: self.owner.remap_definitions(mapper)?,
            visibility: self.visibility,
            function: self.function.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for InherentMethodSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.site.visit_definition_ids(visit, cancel)?;
        self.owner.visit_definition_ids(visit, cancel)?;
        self.function.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for StructSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(StructSignature {
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
            declaration: self.declaration.remap_definitions(mapper)?,
            fields: map_sequence(&self.fields, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for StructSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
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
        self.declaration.visit_definition_ids(visit, cancel)?;
        for value0 in &self.fields {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for NativeTypeSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeTypeSignature {
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
            declaration: self.declaration.remap_definitions(mapper)?,
            representation: self.representation.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for NativeTypeSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
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
        self.declaration.visit_definition_ids(visit, cancel)?;
        self.representation.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for VariantSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(VariantSignature {
            reports_failure: self.reports_failure,
            id: mapper.reference(&self.id)?,
            owner: mapper.reference(&self.owner)?,
            slot: self.slot,
            name: self.name.clone(),
            payload: map_sequence(&self.payload, |value| (value).remap_definitions(mapper))?,
            declaration: self.declaration.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for VariantSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.id)?;
        check_cancel(cancel)?;
        visit(&self.owner)?;
        for value0 in &self.payload {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.declaration.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for EnumSignature {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(EnumSignature {
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
            declaration: self.declaration.remap_definitions(mapper)?,
            variants: map_sequence(&self.variants, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for EnumSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
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
        self.declaration.visit_definition_ids(visit, cancel)?;
        for value0 in &self.variants {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for AggregateCatalog {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
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
                    (value).0.remap_definitions(mapper)?,
                    (value).1.remap_definitions(mapper)?,
                ))
            })?,
            traits: map_entries(
                self.traits.len(),
                self.traits.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).remap_definitions(mapper)?),
                    ))
                }),
            )?,
            implementations: map_entries(
                self.implementations.len(),
                self.implementations.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).remap_definitions(mapper)?),
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
                        Arc::new(((value).as_ref()).remap_definitions(mapper)?),
                    ))
                }),
            )?,
            native_types: map_entries(
                self.native_types.len(),
                self.native_types.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).remap_definitions(mapper)?),
                    ))
                }),
            )?,
            structures: map_entries(
                self.structures.len(),
                self.structures.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        Arc::new(((value).as_ref()).remap_definitions(mapper)?),
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
                        Arc::new(((value).as_ref()).remap_definitions(mapper)?),
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
}

impl DefinitionReferences for AggregateCatalog {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
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
            (value0).0.visit_definition_ids(visit, cancel)?;
            (value0).1.visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.traits {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.implementations {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definition_ids(visit, cancel)?;
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
            ((value0).as_ref()).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.native_types {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definition_ids(visit, cancel)?;
        }
        for (key0, value0) in &self.structures {
            check_cancel(cancel)?;
            visit(key0)?;
            ((value0).as_ref()).visit_definition_ids(visit, cancel)?;
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
            ((value0).as_ref()).visit_definition_ids(visit, cancel)?;
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
