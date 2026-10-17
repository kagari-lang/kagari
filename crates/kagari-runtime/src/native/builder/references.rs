//! Explicit source-trait requirements reuse common declaration and binding checks.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, builder::ModuleBuilder, types::TraitRef},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, metadata::DefinitionMetadata},
};
use kagari_types::declaration::{TraitDef, verify::trait_reference_valid};

impl ModuleBuilder {
    /// Reference a source-defined Kagari trait with its exact expected contract.
    ///
    /// This defines no source trait and exposes no Rust trait automatically. The
    /// defining script module must provide the same checked ABI when loaded.
    /// Explicit `trait_impl` and `bind` supply this receiver's implementation;
    /// source defaults retain their script bodies and associated output contracts.
    /// Invalid identities, native adapter authority, duplicate references and
    /// conflicting contracts are rejected before changing the builder.
    pub fn reference_trait(
        &mut self,
        declaration: &DefinitionPath,
        contract: DefinitionMetadata<TraitDef>,
    ) -> NativeResult<TraitRef> {
        let cancel = CancellationToken::default();
        let metadata = contract
            .import_into(self.context(), &cancel)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        let id = self
            .context()
            .intern(declaration)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        let contract = metadata.into_records();
        for method in &contract.methods {
            self.context()
                .intern_child(id, DefinitionKind::Method, &method.name, 0)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        if !trait_reference_valid(
            &contract,
            id,
            &self.declaration.identity,
            &cancel,
            &self.providers.definitions(),
        ) || self.declaration.trait_references.contains_key(&id)
        {
            return Err(RuntimeError::metadata_conflict(
                "invalid or duplicate source trait reference",
            ));
        }
        self.providers.insert(id, contract.clone())?;
        self.declaration.trait_references.insert(id, contract);
        self.providers.trait_reference(declaration)
    }
}
