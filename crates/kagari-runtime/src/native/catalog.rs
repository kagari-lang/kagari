//! Immutable native contracts used for cross-package authoring and installation.
mod dependencies;
use crate::{error::RuntimeError, native::module::NativeModule};
use kagari_abi::{
    declaration::{ImplDecl, ModuleDecl},
    types::{NativeDeclaration, TraitAbi, TypeAbi, TypeAbiKind},
};
use kagari_common::identity::{DefinitionKind, DefinitionPath};
use std::{collections::BTreeMap, sync::Arc};

/// A declaration view of validated native APIs. It does not install handlers.
#[derive(Debug, Clone, Default)]
pub struct DeclarationCatalog {
    pub(crate) types: Arc<BTreeMap<DefinitionPath, TypeAbi>>,
    pub(crate) traits: Arc<BTreeMap<DefinitionPath, TraitAbi>>,
    pub(crate) declarations: Arc<BTreeMap<DefinitionPath, NativeDeclaration>>,
    pub(crate) implementations: Arc<BTreeMap<DefinitionPath, ImplDecl>>,
}

impl DeclarationCatalog {
    pub fn from_modules(modules: &[&NativeModule]) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        for module in modules {
            result.merge(&module.catalog())?;
        }
        Ok(result)
    }

    pub(crate) fn declared<'a>(
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        for module in modules {
            for ty in &module.types {
                if matches!(ty.kind, TypeAbiKind::NativeStorage(_)) {
                    result.insert_type(
                        module.definition(DefinitionKind::AssociatedType, &ty.name),
                        ty.clone(),
                    )?;
                }
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

    pub(crate) fn get(&self, declaration: &DefinitionPath) -> Option<&TraitAbi> {
        self.traits.get(declaration)
    }

    pub(crate) fn insert(
        &mut self,
        declaration: DefinitionPath,
        contract: TraitAbi,
    ) -> Result<(), RuntimeError> {
        if let Some(previous) = self.traits.get(&declaration) {
            if previous != &contract {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting native trait contracts",
                ));
            }
        } else {
            Arc::make_mut(&mut self.traits).insert(declaration, contract);
        }
        Ok(())
    }

    pub(crate) fn insert_type(
        &mut self,
        id: DefinitionPath,
        declaration: TypeAbi,
    ) -> Result<(), RuntimeError> {
        if let Some(previous) = self.types.get(&id) {
            if previous != &declaration {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting native storage type contracts",
                ));
            }
        } else {
            Arc::make_mut(&mut self.types).insert(id, declaration);
        }
        Ok(())
    }

    pub(crate) fn merge(&mut self, other: &Self) -> Result<(), RuntimeError> {
        for (id, declaration) in other.types.iter() {
            self.insert_type(id.clone(), declaration.clone())?;
        }
        for (declaration, contract) in other.traits.iter() {
            self.insert(declaration.clone(), contract.clone())?;
        }
        for declaration in other.declarations.values() {
            self.insert_declaration(declaration.clone())?;
        }
        for (id, implementation) in other.implementations.iter() {
            self.insert_implementation(id.clone(), implementation.clone())?;
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
            Arc::make_mut(&mut self.implementations).insert(id, implementation);
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
            Arc::make_mut(&mut self.declarations).insert(id.clone(), declaration);
        }
        Ok(())
    }

    pub(crate) fn satisfied_by(&self, installed: &Self) -> bool {
        self.types
            .iter()
            .all(|(id, declaration)| installed.types.get(id) == Some(declaration))
            && self
                .traits
                .iter()
                .all(|(id, contract)| installed.get(id) == Some(contract))
            && self
                .declarations
                .iter()
                .all(|(id, declaration)| installed.declarations.get(id) == Some(declaration))
            && self.implementations.iter().all(|(id, implementation)| {
                installed.implementations.get(id) == Some(implementation)
            })
    }

    pub(crate) fn foreign_to(mut self, owned: &Self) -> Self {
        Arc::make_mut(&mut self.types).retain(|id, _| !owned.types.contains_key(id));
        Arc::make_mut(&mut self.traits).retain(|id, _| !owned.traits.contains_key(id));
        Arc::make_mut(&mut self.declarations).retain(|id, _| !owned.declarations.contains_key(id));
        Arc::make_mut(&mut self.implementations)
            .retain(|id, _| !owned.implementations.contains_key(id));
        self
    }

    pub(crate) fn check_implementations<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<(), RuntimeError> {
        for module in modules {
            module
                .validate_trait_implementations(&self.traits)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        Ok(())
    }
}
