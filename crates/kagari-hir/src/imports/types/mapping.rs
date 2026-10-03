//! Explicit traversal of the owning HIR records.
use crate::identity_mapping::map_hash_entries;
use crate::imports::types::{ImportedTraitMethod, ImportedType, ImportedTypes};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for ImportedType<I> {
    type Rebind<J: DefinitionReference> = ImportedType<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedType {
            native_type: self
                .native_type
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            associated_arities: self.associated_arities.clone(),
            id: self.id,
            declaration: self.declaration.map_identities(mapper)?,
            ty: self.ty.map_identities(mapper)?,
            trait_methods: map_sequence(&self.trait_methods, |value| {
                (value).map_identities(mapper)
            })?,
            associated_types: self.associated_types.clone(),
            supertraits: map_sequence(&self.supertraits, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.native_type.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.declaration.visit_definitions(visit, cancel)?;
        self.ty.visit_definitions(visit, cancel)?;
        for value0 in &self.trait_methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.supertraits {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ImportedTraitMethod<I> {
    type Rebind<J: DefinitionReference> = ImportedTraitMethod<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedTraitMethod {
            name: self.name.clone(),
            declaration: mapper.reference(&self.declaration)?,
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
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ImportedTypes<I> {
    type Rebind<J: DefinitionReference> = ImportedTypes<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedTypes {
            types: map_hash_entries(
                self.types.len(),
                self.types
                    .iter()
                    .map(|(key, value)| Ok(((key).clone(), (value).map_identities(mapper)?))),
            )?,
            resolutions: map_hash_entries(
                self.resolutions.len(),
                self.resolutions
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            nominal_types: map_hash_entries(
                self.nominal_types.len(),
                self.nominal_types.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
            variants: map_hash_entries(
                self.variants.len(),
                self.variants
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in self.types.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.resolutions.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.nominal_types {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.variants.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
