//! Explicit traversal of the owning HIR records.
use crate::aggregates::implementations::ImplementationSignature;
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

impl<I: DefinitionReference> DefinitionRecord<I> for ImplementationSignature<I> {
    type Rebind<J: DefinitionReference> = ImplementationSignature<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImplementationSignature {
            engine_owned: self.engine_owned,
            associated_type_families: map_entries(
                self.associated_type_families.len(),
                self.associated_type_families.iter().map(|(key, value)| {
                    Ok((mapper.reference(key)?, (value).map_identities(mapper)?))
                }),
            )?,
            id: mapper.reference(&self.id)?,
            trait_type: self.trait_type.map_identities(mapper)?,
            for_type: self.for_type.map_identities(mapper)?,
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
            methods: map_entries(
                self.methods.len(),
                self.methods
                    .iter()
                    .map(|(key, value)| Ok((mapper.reference(key)?, mapper.reference(value)?))),
            )?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (key0, value0) in &self.associated_type_families {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.id)?;
        self.trait_type.visit_definitions(visit, cancel)?;
        self.for_type.visit_definitions(visit, cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.bounds {
            (key0).visit_definitions(visit, cancel)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        for (key0, value0) in &self.methods {
            check_cancel(cancel)?;
            visit(key0)?;
            check_cancel(cancel)?;
            visit(value0)?;
        }
        Ok(())
    }
}
