//! Immutable native contracts used for cross-package authoring and installation.
mod dependencies;
pub(crate) mod import;
mod ownership;
use crate::error::RuntimeError;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath,
    map::{DefinitionContext, DefinitionMap},
    reference::DefinitionReference,
    table::{DefinitionId, DefinitionTableError},
};
use kagari_contract::{
    declaration::{ImplDecl, ModuleDecl},
    types::{NativeDeclaration, TraitDef, TypeDef, TypeDefKind},
};
use std::{collections::BTreeMap, sync::Arc};

/// A declaration view of validated native APIs. It does not install handlers.
#[derive(Debug, Clone)]
pub struct DeclarationCatalog<I = DefinitionId> {
    pub(crate) types: Arc<DefinitionMap<TypeDef<I>>>,
    pub(crate) traits: Arc<DefinitionMap<TraitDef<I>>>,
    pub(crate) declarations: Arc<DefinitionMap<NativeDeclaration<I>>>,
    pub(crate) implementations: Arc<DefinitionMap<ImplDecl<I>>>,
}

impl<I> Default for DeclarationCatalog<I> {
    fn default() -> Self {
        let context = DefinitionContext::new().expect("definition context identity exhausted");
        Self {
            types: Arc::new(DefinitionMap::new(context.clone())),
            traits: Arc::new(DefinitionMap::new(context.clone())),
            declarations: Arc::new(DefinitionMap::new(context.clone())),
            implementations: Arc::new(DefinitionMap::new(context)),
        }
    }
}

impl DeclarationCatalog<DefinitionPath> {
    pub(crate) fn collect_authoring<'a>(
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        for module in modules {
            for ty in &module.types {
                result.insert_type(
                    module.definition(
                        match ty.kind {
                            TypeDefKind::Native(constructor) => constructor.declaration_kind(),
                            _ => DefinitionKind::AssociatedType,
                        },
                        &ty.name,
                    ),
                    ty.clone(),
                )?;
            }
            for contract in &module.traits {
                result.insert(
                    module.definition(DefinitionKind::Trait, &contract.name),
                    contract.clone(),
                )?;
            }
            for declaration in module.native_declarations() {
                result.insert_declaration(declaration)?;
            }
            for (index, implementation) in module.implementations.iter().enumerate() {
                if implementation.trait_type.is_some() {
                    result.insert_implementation(
                        module.implementation_id(index),
                        implementation.clone(),
                    )?;
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn get(&self, declaration: &DefinitionPath) -> Option<&TraitDef> {
        self.traits.get(declaration)
    }

    pub(crate) fn insert(
        &mut self,
        declaration: DefinitionPath,
        contract: TraitDef,
    ) -> Result<(), RuntimeError> {
        if let Some(previous) = self.traits.get(&declaration) {
            if previous != &contract {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting native trait contracts",
                ));
            }
        } else {
            Arc::make_mut(&mut self.traits)
                .insert(declaration, contract)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn insert_type(
        &mut self,
        id: DefinitionPath,
        declaration: TypeDef,
    ) -> Result<(), RuntimeError> {
        if let Some(previous) = self.types.get(&id) {
            if previous != &declaration {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting native storage type contracts",
                ));
            }
        } else {
            Arc::make_mut(&mut self.types)
                .insert(id, declaration)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn insert_implementation(
        &mut self,
        id: DefinitionPath,
        implementation: ImplDecl,
    ) -> Result<(), RuntimeError> {
        if let Some(previous) = self.implementations.get(&id) {
            if previous != &implementation {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting native implementation contracts",
                ));
            }
        } else {
            if self.implementations.len() >= 4096 {
                return Err(RuntimeError::metadata_conflict(
                    "native implementation catalog exceeds proof limits",
                ));
            }
            Arc::make_mut(&mut self.implementations)
                .insert(id, implementation)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn insert_declaration(
        &mut self,
        declaration: NativeDeclaration,
    ) -> Result<(), RuntimeError> {
        let id = &declaration.declaration;
        if let Some(previous) = self.declarations.get(id) {
            if previous != &declaration {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting native template declarations",
                ));
            }
        } else {
            Arc::make_mut(&mut self.declarations)
                .insert(id.clone(), declaration)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn check_implementations<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<(), RuntimeError> {
        let traits: BTreeMap<_, _> = self
            .traits
            .iter()
            .map(|(id, contract)| (id, contract.clone()))
            .collect();
        for module in modules {
            module
                .validate_trait_implementations(&traits)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DeclarationCatalog<I> {
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
