//! Scoped raw declarations join the same batch, without becoming checked providers.
use crate::native::{
    assembly::{authoring::PendingBinding, conflict, draft::ModuleDraft},
    binding::NativeResult,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{metadata::DefinitionMetadata, table::DefinitionId},
};
use kagari_types::declaration::module::ModuleDecl;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

impl ModuleDraft<'_> {
    /// Seed an empty owner with raw declarations and deferred direct associations.
    /// This adopts identities, not validation evidence. Further local contributors
    /// may append declarations; all contracts, coverage and storage are checked by
    /// ordinary batch sealing, and adapters remain unprepared.
    pub fn import_declaration(
        &mut self,
        declaration: &DefinitionMetadata<ModuleDecl>,
        bindings: BTreeMap<DefinitionId, PendingBinding>,
    ) -> NativeResult<()> {
        let current = &self.state.declaration;
        if declaration.records().identity != *self.identity()
            || current.prelude
            || current.package_alias.is_some()
            || !current.dependencies.is_empty()
            || !current.types.is_empty()
            || !current.paths.is_empty()
            || !current.variant_exports.is_empty()
            || !current.exports.is_empty()
            || !current.traits.is_empty()
            || !current.trait_references.is_empty()
            || !current.implementations.is_empty()
            || !current.functions.is_empty()
            || !current.private_functions.is_empty()
            || !current.module_documentation.is_empty()
            || !current.documentation.is_empty()
            || !current.callable_requirements.is_empty()
            || !current.concrete_results.is_empty()
            || !self.state.bindings.is_empty()
            || !self.state.storage.is_empty()
        {
            return Err(conflict(
                "raw declaration import requires an empty matching module owner",
            ));
        }
        let imported = declaration
            .import_into(self.context, &CancellationToken::default())
            .map_err(conflict)?
            .into_records();
        let remap = self
            .context
            .import(declaration.definitions(), bindings.keys().copied())
            .map_err(conflict)?;
        let bindings = bindings
            .into_iter()
            .map(|(id, binding)| Ok((remap.map(id).map_err(conflict)?, binding)))
            .collect::<NativeResult<_>>()?;
        imported
            .collect_definitions(self.context)
            .map_err(conflict)?;
        self.state.declaration = imported;
        self.state.bindings = bindings;
        Ok(())
    }
}
