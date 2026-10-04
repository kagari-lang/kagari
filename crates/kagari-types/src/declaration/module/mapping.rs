//! Contextual identity traversal of the owning metadata records.
use crate::declaration::module::{ImplDecl, ModuleDecl};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_entries,
            map_sequence, map_set,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for ImplDecl<I> {
    type Rebind<J: DefinitionReference> = ImplDecl<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ImplDecl {
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
            trait_type: self
                .trait_type
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            for_type: self.for_type.map_identities(mapper)?,
            methods: map_sequence(&self.methods, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        if let Some(value0) = self.trait_type.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.for_type.visit_definitions(visit, cancel)?;
        for value0 in &self.methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ModuleDecl<I> {
    type Rebind<J: DefinitionReference> = ModuleDecl<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ModuleDecl {
            identity: self.identity.clone(),
            package_alias: self.package_alias.clone(),
            dependencies: self.dependencies.clone(),
            types: map_sequence(&self.types, |value| (value).map_identities(mapper))?,
            variant_exports: self.variant_exports.clone(),
            exports: map_entries(
                self.exports.len(),
                self.exports
                    .iter()
                    .map(|(name, id)| Ok((name.clone(), mapper.reference(id)?))),
            )?,
            traits: map_sequence(&self.traits, |value| (value).map_identities(mapper))?,
            implementations: map_sequence(&self.implementations, |value| {
                (value).map_identities(mapper)
            })?,
            functions: map_sequence(&self.functions, |value| (value).map_identities(mapper))?,
            private_functions: map_set(
                self.private_functions.len(),
                self.private_functions
                    .iter()
                    .map(|value| mapper.reference(value)),
            )?,
            documentation: map_entries(
                self.documentation.len(),
                self.documentation
                    .iter()
                    .map(|(key, value)| Ok((mapper.reference(key)?, (value).clone()))),
            )?,
            callable_requirements: map_entries(
                self.callable_requirements.len(),
                self.callable_requirements.iter().map(|(key, value)| {
                    Ok((
                        mapper.reference(key)?,
                        map_sequence(value, |value| (value).map_identities(mapper))?,
                    ))
                }),
            )?,
            concrete_results: map_entries(
                self.concrete_results.len(),
                self.concrete_results.iter().map(|(key, value)| {
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
        for value0 in &self.types {
            (value0).visit_definitions(visit, cancel)?;
        }
        for id in self.exports.values() {
            visit(id)?;
        }
        for value0 in &self.traits {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.implementations {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.functions {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.private_functions {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        for key0 in self.documentation.keys() {
            check_cancel(cancel)?;
            visit(key0)?;
        }
        for (key0, value0) in &self.callable_requirements {
            check_cancel(cancel)?;
            visit(key0)?;
            for value1 in value0 {
                (value1).visit_definitions(visit, cancel)?;
            }
        }
        for (key0, value0) in &self.concrete_results {
            check_cancel(cancel)?;
            visit(key0)?;
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
