//! Immutable native contracts used for cross-package authoring and installation.
mod dependencies;
pub(crate) mod import;
mod ownership;
mod paths;
mod proofs;
mod rust_types;
mod validation;
use crate::{error::RuntimeError, native::catalog::rust_types::RustTypes};
use kagari_common::identity::{
    ModuleIdentity,
    map::{DefinitionContext, DefinitionMap},
    table::{DefinitionId, DefinitionTableError},
};
use kagari_types::declaration::path::HostPathDeclaration;
use kagari_types::declaration::{NativeDeclaration, TraitDef, TypeDef, module::ImplDecl};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// A declaration view of validated native APIs. It does not install handlers.
#[derive(Debug, Clone)]
pub struct DeclarationCatalog {
    /// Actual module providers, including empty and re-export-only modules. Partial
    /// dependency records and interned references do not establish module ownership.
    /// This authoring inventory is separate from executable contract requirements.
    pub(super) module_owners: Arc<BTreeSet<ModuleIdentity>>,
    pub(crate) rust_types: RustTypes,
    /// Authoring-only Markdown; excluded from executable contract comparisons.
    pub(crate) documentation: Arc<BTreeMap<DefinitionId, String>>,
    pub(crate) types: Arc<DefinitionMap<TypeDef>>,
    pub(crate) traits: Arc<DefinitionMap<TraitDef>>,
    pub(crate) declarations: Arc<DefinitionMap<NativeDeclaration>>,
    pub(crate) implementations: Arc<DefinitionMap<ImplDecl>>,
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
            rust_types: Arc::default(),
            documentation: Arc::default(),
            types: Arc::new(DefinitionMap::new(context.clone())),
            traits: Arc::new(DefinitionMap::new(context.clone())),
            declarations: Arc::new(DefinitionMap::new(context.clone())),
            implementations: Arc::new(DefinitionMap::new(context.clone())),
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
            Arc::make_mut(&mut self.paths).remove_keys(&owned.paths)?;
            Ok(())
        };
        let mut remove = remove;
        remove().map_err(|error: DefinitionTableError| {
            RuntimeError::metadata_conflict(error.to_string())
        })?;
        Arc::make_mut(&mut self.module_owners).retain(|owner| !owned.module_owners.contains(owner));
        Ok(self)
    }
}
