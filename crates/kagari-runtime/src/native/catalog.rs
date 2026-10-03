//! Immutable native contracts used for cross-package authoring and installation.
mod dependencies;
use crate::{error::RuntimeError, native::module::NativeModule};
use kagari_abi::{
    declaration::{ImplDecl, ModuleDecl},
    types::{NativeDeclaration, TraitAbi, TypeAbi, TypeAbiKind},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath,
    map::{DefinitionContext, DefinitionMap},
    table::DefinitionTableError,
};
use std::{collections::BTreeMap, sync::Arc};

/// A declaration view of validated native APIs. It does not install handlers.
#[derive(Debug, Clone)]
pub struct DeclarationCatalog {
    pub(crate) types: Arc<DefinitionMap<TypeAbi>>,
    pub(crate) traits: Arc<DefinitionMap<TraitAbi>>,
    pub(crate) declarations: Arc<DefinitionMap<NativeDeclaration>>,
    pub(crate) implementations: Arc<DefinitionMap<ImplDecl>>,
}

impl Default for DeclarationCatalog {
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
            Arc::make_mut(&mut self.traits)
                .insert(declaration, contract)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
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
            Arc::make_mut(&mut self.types)
                .insert(id, declaration)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn merge(&mut self, other: &Self) -> Result<(), RuntimeError> {
        if self
            .implementations
            .union_len(&other.implementations)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
            > 4096
        {
            return Err(RuntimeError::metadata_conflict(
                "native implementation catalog exceeds proof limits",
            ));
        }
        merge_index(
            &mut self.types,
            &other.types,
            "conflicting native storage type contracts",
        )?;
        merge_index(
            &mut self.traits,
            &other.traits,
            "conflicting native trait contracts",
        )?;
        merge_index(
            &mut self.declarations,
            &other.declarations,
            "conflicting native template declarations",
        )?;
        merge_index(
            &mut self.implementations,
            &other.implementations,
            "conflicting native implementation contracts",
        )?;
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

    pub(crate) fn satisfied_by(&self, installed: &Self) -> Result<bool, RuntimeError> {
        let check = || -> Result<_, _> {
            Ok(self.types.is_subset_of(&installed.types)?
                && self.traits.is_subset_of(&installed.traits)?
                && self.declarations.is_subset_of(&installed.declarations)?
                && self
                    .implementations
                    .is_subset_of(&installed.implementations)?)
        };
        check().map_err(|error: DefinitionTableError| {
            RuntimeError::metadata_conflict(error.to_string())
        })
    }

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

fn merge_index<T: Clone + PartialEq>(
    target: &mut Arc<DefinitionMap<T>>,
    source: &DefinitionMap<T>,
    conflict: &str,
) -> Result<(), RuntimeError> {
    if source
        .is_subset_of(target)
        .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
    {
        return Ok(());
    }
    if !Arc::make_mut(target)
        .merge(source)
        .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
    {
        return Err(RuntimeError::metadata_conflict(conflict));
    }
    Ok(())
}
