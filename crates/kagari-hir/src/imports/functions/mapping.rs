//! Explicit traversal of the owning HIR records.
use crate::identity_mapping::map_hash_entries;
use crate::imports::functions::{ImportedFunction, ImportedFunctions};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel},
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for ImportedFunction<I> {
    type Rebind<J: DefinitionReference> = ImportedFunction<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedFunction {
            id: self.id,
            declaration: mapper.reference(&self.declaration)?,
            site: self.site.map_identities(mapper)?,
            signature: self.signature.map_identities(mapper)?,
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
        self.signature.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ImportedFunctions<I> {
    type Rebind<J: DefinitionReference> = ImportedFunctions<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImportedFunctions {
            functions: map_hash_entries(
                self.functions.len(),
                self.functions
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).map_identities(mapper)?))),
            )?,
            methods: map_hash_entries(
                self.methods.len(),
                self.methods.iter().map(|(key, value)| {
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
        for value0 in self.functions.values() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for (key0, value0) in &self.methods {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
