//! Immutable native trait contracts used for cross-package authoring and installation.
use crate::{error::RuntimeError, native::api::NativeApi};
use kagari_abi::{native_api::NativeModule, types::TraitAbi};
use kagari_common::identity::{DefinitionId, DefinitionKind};
use std::collections::{BTreeMap, HashSet};

/// A declaration view of validated native APIs. It does not install handlers.
#[derive(Debug, Clone, Default)]
pub struct NativeCatalog {
    pub(crate) traits: BTreeMap<DefinitionId, TraitAbi>,
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
            self.traits.insert(declaration, contract);
        }
        Ok(())
    }

    pub(crate) fn merge(&mut self, other: &Self) -> Result<(), RuntimeError> {
        for (declaration, contract) in &other.traits {
            self.insert(declaration.clone(), contract.clone())?;
        }
        Ok(())
    }

    /// Retain the selected contract and its declared parent contracts. Missing
    /// parents cannot become implicit installation authority.
    pub(crate) fn selected(&self, declaration: &DefinitionId) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        let mut pending = vec![declaration.clone()];
        let mut seen = HashSet::new();
        while let Some(declaration) = pending.pop() {
            if !seen.insert(declaration.clone()) {
                continue;
            }
            let contract = self.get(&declaration).ok_or_else(|| {
                RuntimeError::metadata_conflict(
                    "native trait dependency is absent from the catalog",
                )
            })?;
            pending.extend(
                contract
                    .supertraits
                    .iter()
                    .map(|parent| parent.declaration.clone()),
            );
            result.insert(declaration, contract.clone())?;
        }
        Ok(result)
    }

    pub(crate) fn satisfied_by(&self, installed: &Self) -> bool {
        self.traits
            .iter()
            .all(|(id, contract)| installed.get(id) == Some(contract))
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
