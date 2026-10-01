//! Immutable native contracts used for cross-package authoring and installation.
mod dependencies;
use crate::{error::RuntimeError, native::api::NativeApi};
use kagari_abi::{
    native_api::NativeModule,
    types::{NativeDeclaration, TraitAbi},
};
use kagari_common::identity::{DefinitionId, DefinitionKind};
use std::{collections::BTreeMap, sync::Arc};

/// A declaration view of validated native APIs. It does not install handlers.
#[derive(Debug, Clone, Default)]
pub struct NativeCatalog {
    pub(crate) traits: Arc<BTreeMap<DefinitionId, TraitAbi>>,
    pub(crate) declarations: Arc<BTreeMap<DefinitionId, NativeDeclaration>>,
}

impl NativeCatalog {
    pub fn from_apis(apis: &[&NativeApi]) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        for api in apis {
            result.merge(&api.catalog())?;
        }
        Ok(result)
    }

    pub(crate) fn declared<'a>(
        modules: impl IntoIterator<Item = &'a NativeModule>,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        for module in modules {
            for contract in &module.traits {
                result.insert(
                    module.definition(DefinitionKind::Trait, &contract.name),
                    contract.clone(),
                )?;
            }
            for declaration in module.native_declarations() {
                result.insert_declaration(declaration)?;
            }
        }
        Ok(result)
    }

    pub(crate) fn get(&self, declaration: &DefinitionId) -> Option<&TraitAbi> {
        self.traits.get(declaration)
    }

    pub(crate) fn insert(
        &mut self,
        declaration: DefinitionId,
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

    pub(crate) fn merge(&mut self, other: &Self) -> Result<(), RuntimeError> {
        for (declaration, contract) in other.traits.iter() {
            self.insert(declaration.clone(), contract.clone())?;
        }
        for declaration in other.declarations.values() {
            self.insert_declaration(declaration.clone())?;
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
        self.traits
            .iter()
            .all(|(id, contract)| installed.get(id) == Some(contract))
            && self
                .declarations
                .iter()
                .all(|(id, declaration)| installed.declarations.get(id) == Some(declaration))
    }

    pub(crate) fn foreign_to(mut self, owned: &Self) -> Self {
        Arc::make_mut(&mut self.traits).retain(|id, _| !owned.traits.contains_key(id));
        Arc::make_mut(&mut self.declarations).retain(|id, _| !owned.declarations.contains_key(id));
        self
    }

    pub(crate) fn check_implementations<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a NativeModule>,
    ) -> Result<(), RuntimeError> {
        for module in modules {
            module
                .validate_trait_implementations(&self.traits)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        Ok(())
    }
}
