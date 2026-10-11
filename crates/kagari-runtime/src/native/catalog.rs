//! Immutable native contracts used for cross-package authoring and installation.
mod dependencies;
pub(crate) mod import;
mod ownership;
mod proofs;
use crate::error::RuntimeError;
use kagari_common::identity::{
    DefinitionPath,
    map::{DefinitionContext, DefinitionMap},
    table::{DefinitionId, DefinitionTableError},
};
use kagari_types::declaration::{NativeDeclaration, TraitDef, TypeDef, module::ImplDecl};
use std::{collections::BTreeMap, sync::Arc};

/// A declaration view of validated native APIs. It does not install handlers.
#[derive(Debug, Clone)]
pub struct DeclarationCatalog {
    /// Authoring-only Markdown; excluded from executable contract comparisons.
    pub(crate) documentation: Arc<BTreeMap<DefinitionPath, String>>,
    pub(crate) types: Arc<DefinitionMap<TypeDef<DefinitionId>>>,
    pub(crate) traits: Arc<DefinitionMap<TraitDef<DefinitionId>>>,
    pub(crate) declarations: Arc<DefinitionMap<NativeDeclaration<DefinitionId>>>,
    pub(crate) implementations: Arc<DefinitionMap<ImplDecl<DefinitionId>>>,
}

impl Default for DeclarationCatalog {
    fn default() -> Self {
        Self::in_context(DefinitionContext::new().expect("definition context identity exhausted"))
    }
}

impl DeclarationCatalog {
    fn in_context(context: DefinitionContext) -> Self {
        Self {
            documentation: Arc::default(),
            types: Arc::new(DefinitionMap::new(context.clone())),
            traits: Arc::new(DefinitionMap::new(context.clone())),
            declarations: Arc::new(DefinitionMap::new(context.clone())),
            implementations: Arc::new(DefinitionMap::new(context)),
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
            Ok(())
        };
        let mut remove = remove;
        remove().map_err(|error: DefinitionTableError| {
            RuntimeError::metadata_conflict(error.to_string())
        })?;
        Ok(self)
    }
}
