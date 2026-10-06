//! Explicit traversal of the owning HIR records.
use crate::declarations::{Declaration, DeclarationId, Declarations};
use crate::identity_mapping::map_hash_entries;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel},
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for DeclarationId<I> {
    type Rebind<J: DefinitionReference> = DeclarationId<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Definition(field0) => DeclarationId::Definition(mapper.reference(field0)?),
            Self::GenericParameter { owner, position } => DeclarationId::GenericParameter {
                owner: mapper.reference(owner)?,
                position: *(position),
            },
            Self::Binding(field0) => DeclarationId::Binding(field0.clone()),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Definition(field0) => {
                check_cancel(cancel)?;
                visit(field0)?;
            }
            Self::GenericParameter { owner, position: _ } => {
                check_cancel(cancel)?;
                visit(owner)?;
            }
            Self::Binding(_) => {}
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for Declaration<I> {
    type Rebind<J: DefinitionReference> = Declaration<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(Declaration {
            id: self.id.map_identities(mapper)?,
            name: self.name.clone(),
            location: self.location,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.id.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for Declarations<I> {
    type Rebind<J: DefinitionReference> = Declarations<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(Declarations {
            array_interfaces: self
                .array_interfaces
                .iter()
                .map(|(access, id)| Ok((*access, mapper.reference(id)?)))
                .collect::<Result<_, DefinitionMappingError>>()?,
            language_items: self
                .language_items
                .iter()
                .map(|(role, id)| Ok((*role, mapper.reference(id)?)))
                .collect::<Result<_, DefinitionMappingError>>()?,
            imported_types: self.imported_types.map_identities(mapper)?,
            names: self.names.clone(),
            hosts: self.hosts.clone(),
            imports: self.imports.clone(),
            catalog: self.catalog.clone(),
            analysis: self.analysis,
            definitions: self.definitions.clone(),
            context: self.context.clone(),
            targets: map_hash_entries(
                self.targets.len(),
                self.targets
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), (value).map_identities(mapper)?))),
            )?,
            identities: map_hash_entries(
                self.identities.len(),
                self.identities
                    .iter()
                    .map(|(key, value)| Ok(((key).map_identities(mapper)?, value.clone()))),
            )?,
            site_ranges: self.site_ranges.clone(),
            impl_identities: map_hash_entries(
                self.impl_identities.len(),
                self.impl_identities
                    .iter()
                    .map(|(key, value)| Ok((*key, mapper.reference(value)?))),
            )?,
            native_types: map_hash_entries(
                self.native_types.len(),
                self.native_types
                    .iter()
                    .map(|(key, value)| Ok((*key, (value).map_identities(mapper)?))),
            )?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for id in self.array_interfaces.values() {
            check_cancel(cancel)?;
            visit(id)?;
        }
        for id in self.language_items.values() {
            check_cancel(cancel)?;
            visit(id)?;
        }
        self.imported_types.visit_definitions(visit, cancel)?;
        for value0 in self.targets.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for key0 in self.identities.keys() {
            (key0).visit_definitions(visit, cancel)?;
        }
        for value0 in self.impl_identities.values() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        for value0 in self.native_types.values() {
            (value0).visit_definitions(visit, cancel)?;
        }

        Ok(())
    }
}
