//! Immutable native contracts used for cross-package authoring and installation.
mod authored;
#[cfg(test)]
mod authored_tests;
mod dependencies;
pub(crate) mod import;
mod ownership;
mod paths;
mod projection;
mod proofs;
mod rust_types;
mod validation;
use crate::{error::RuntimeError, native::catalog::rust_types::RustTypes};
use kagari_bytecode::program::verified::VerifiedProductIdentity;
use kagari_common::identity::{
    ModuleIdentity,
    map::{DefinitionContext, DefinitionMap},
    table::{DefinitionId, DefinitionTableError},
};
use kagari_contract::types::InterfaceTable;
use kagari_types::declaration::path::HostPathDeclaration;
use kagari_types::declaration::{NativeDeclaration, TraitDef, TypeDef, module::ImplDecl};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Scoped contracts for native authoring. Neither a declaration nor its origin installs a handler.
#[derive(Debug, Clone)]
pub struct DeclarationCatalog {
    /// Actual module providers, including empty and re-export-only modules. Partial
    /// dependency records and interned references do not establish module ownership.
    /// This authoring inventory is separate from executable contract requirements.
    module_owners: Arc<BTreeSet<ModuleIdentity>>,
    /// Origin of verified module contracts, including requirements in a partial view.
    /// Unlike `module_owners`, a required origin does not publish a complete owner.
    authored: Arc<BTreeMap<ModuleIdentity, VerifiedProductIdentity>>,
    pub(crate) rust_types: RustTypes,
    /// Authoring-only Markdown; excluded from executable contract comparisons.
    pub(crate) documentation: Arc<BTreeMap<DefinitionId, String>>,
    pub(crate) types: Arc<DefinitionMap<TypeDef>>,
    pub(crate) traits: Arc<DefinitionMap<TraitDef>>,
    pub(crate) declarations: Arc<DefinitionMap<NativeDeclaration>>,
    pub(crate) implementations: Arc<DefinitionMap<ImplDecl>>,
    /// Script implementation ABI only: no executable bodies, native entries or dispatch bridges.
    pub(crate) source_impls: Arc<DefinitionMap<InterfaceTable>>,
    /// Path contracts belong to their external root's declaring module.
    pub(crate) paths: Arc<DefinitionMap<Vec<HostPathDeclaration>>>,
}

impl Default for DeclarationCatalog {
    fn default() -> Self {
        Self::in_context(DefinitionContext::new().expect("definition context identity exhausted"))
    }
}

impl DeclarationCatalog {
    pub(crate) fn in_context(context: DefinitionContext) -> Self {
        Self {
            module_owners: Arc::default(),
            authored: Arc::default(),
            rust_types: Arc::default(),
            documentation: Arc::default(),
            types: Arc::new(DefinitionMap::new(context.clone())),
            traits: Arc::new(DefinitionMap::new(context.clone())),
            declarations: Arc::new(DefinitionMap::new(context.clone())),
            implementations: Arc::new(DefinitionMap::new(context.clone())),
            source_impls: Arc::new(DefinitionMap::new(context.clone())),
            paths: Arc::new(DefinitionMap::new(context)),
        }
    }
}

impl DeclarationCatalog {
    pub(crate) fn foreign_to(mut self, owned: &Self) -> Result<Self, RuntimeError> {
        let remove = || -> Result<_, _> {
            Arc::make_mut(&mut self.types).remove_keys(&owned.types)?;
            Arc::make_mut(&mut self.traits).remove_keys(&owned.traits)?;
            Arc::make_mut(&mut self.declarations).remove_keys(&owned.declarations)?;
            Arc::make_mut(&mut self.implementations).remove_keys(&owned.implementations)?;
            if !owned.source_impls.is_empty() {
                Arc::make_mut(&mut self.source_impls).remove_keys(&owned.source_impls)?;
            }
            Arc::make_mut(&mut self.paths).remove_keys(&owned.paths)?;
            Ok(())
        };
        let mut remove = remove;
        remove().map_err(|error: DefinitionTableError| {
            RuntimeError::metadata_conflict(error.to_string())
        })?;
        Arc::make_mut(&mut self.module_owners).retain(|owner| !owned.module_owners.contains(owner));
        if !owned.authored.is_empty() {
            let definitions = self.definitions();
            let remaining = self
                .types
                .ids()
                .chain(self.traits.ids())
                .chain(self.source_impls.ids())
                .map(|id| definitions.resolve(id).map(|view| view.module()))
                .collect::<Result<BTreeSet<_>, _>>()
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
            Arc::make_mut(&mut self.authored).retain(|owner, _| {
                !owned.authored.contains_key(owner)
                    || self.module_owners.contains(owner)
                    || remaining.contains(owner)
            });
        }
        Ok(self)
    }
}
