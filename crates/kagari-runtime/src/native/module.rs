//! Checked declarations and local Rust entries form one installable module.
use crate::{
    Runtime,
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        registry::{BindingRegistration, NativeRegistry},
        storage::NativeStorage,
    },
};
use kagari_abi::{
    callable::CallableImplementation,
    declaration::{ModuleDecl, render::DeclarationSource},
    types::TypeAbiKind,
};
use kagari_common::identity::{DefinitionId, DefinitionKind};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone)]
pub struct NativeModule {
    declaration: Arc<ModuleDecl>,
    bindings: Vec<BindingRegistration>,
    required: DeclarationCatalog,
    storage: BTreeMap<DefinitionId, NativeStorage>,
}
impl NativeModule {
    pub(crate) fn checked(
        declaration: ModuleDecl,
        bindings: Vec<(DefinitionId, NativeBinding)>,
        storage: BTreeMap<DefinitionId, NativeStorage>,
        providers: &DeclarationCatalog,
    ) -> NativeResult<Self> {
        declaration
            .validate()
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        let mut entries = BTreeMap::new();
        for declaration in declaration.native_declarations() {
            let CallableImplementation::Native(binding) = &declaration.function.implementation
            else {
                return Err(RuntimeError::metadata_conflict(
                    "native method has no Rust binding",
                ));
            };
            entries
                .entry(binding.clone())
                .or_insert_with(Vec::new)
                .push(declaration);
        }
        let owned = DeclarationCatalog::declared([&declaration])?;
        if owned.types.len() != storage.len()
            || owned.types.iter().any(|(id, ty)| {
                storage
                    .get(id)
                    .is_none_or(|storage| ty.kind != TypeAbiKind::NativeStorage(storage.layout()))
            })
        {
            return Err(RuntimeError::metadata_conflict(
                "native storage entries differ from their declared types",
            ));
        }
        let mut available = owned.clone();
        available.merge(providers)?;
        let dependencies = available.dependencies(
            owned.traits.keys(),
            owned.declarations.values(),
            [&declaration],
        )?;
        let mut checked = owned.clone();
        checked.merge(&dependencies)?;
        checked.check_implementations([&declaration])?;
        checked.validate_defaults()?;
        let mut registrations = Vec::new();
        let mut registry = NativeRegistry::default();
        for (id, binding) in bindings {
            let declarations = entries.remove(&id).ok_or_else(|| {
                RuntimeError::metadata_conflict("unknown or duplicate native binding")
            })?;
            let required_catalog =
                checked.dependencies(owned.traits.keys(), &declarations, [&declaration])?;
            let registration = BindingRegistration {
                declarations,
                binding,
                required_catalog,
            };
            registry.install(registration.clone())?;
            registrations.push(registration);
        }
        if !entries.is_empty() {
            return Err(RuntimeError::metadata_conflict(
                "missing native implementation binding",
            ));
        }
        let required = dependencies.foreign_to(&owned);
        let mut declaration = declaration;
        declaration.dependencies = required
            .traits
            .keys()
            .chain(required.types.keys())
            .chain(required.declarations.keys())
            .chain(required.implementations.keys())
            .map(|id| id.module.clone())
            .filter(|owner| *owner != declaration.identity)
            .collect();
        declaration
            .validate()
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        Ok(Self {
            declaration: Arc::new(declaration),
            bindings: registrations,
            required,
            storage,
        })
    }
    pub fn declaration(&self) -> &Arc<ModuleDecl> {
        &self.declaration
    }
    pub fn declaration_source(&self) -> DeclarationSource {
        self.declaration
            .declaration_source()
            .expect("checked module presentation")
    }
    pub fn catalog(&self) -> DeclarationCatalog {
        let mut catalog = DeclarationCatalog::declared([self.declaration.as_ref()])
            .expect("checked module declarations");
        catalog
            .merge(&self.required)
            .expect("checked module dependencies");
        catalog
    }
    pub fn install(&self, runtime: &mut Runtime) -> NativeResult<()> {
        self.install_into(&mut runtime.native_entries)
    }
    pub(crate) fn install_into(&self, registry: &mut NativeRegistry) -> NativeResult<()> {
        let mut staged = registry.clone();
        if !self.required.satisfied_by(&staged.catalog) {
            return Err(RuntimeError::metadata_conflict(
                "native module dependency is not installed",
            ));
        }
        let owned = DeclarationCatalog::declared([self.declaration.as_ref()])?;
        for id in owned.traits.keys() {
            if staged.catalog.get(id).is_some() {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native trait owner",
                ));
            }
        }
        for (id, storage) in &self.storage {
            if staged.storage.insert(id.clone(), storage.clone()).is_some() {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native storage owner",
                ));
            }
        }
        staged.catalog.merge(&owned)?;
        staged
            .catalog
            .check_implementations([self.declaration.as_ref()])?;
        for registration in &self.bindings {
            staged.install(registration.clone())?;
        }
        staged.catalog.validate_defaults()?;
        *registry = staged;
        Ok(())
    }
    pub fn trait_id(&self, name: &str) -> NativeResult<DefinitionId> {
        self.declaration
            .traits
            .iter()
            .any(|contract| contract.name == name)
            .then(|| self.declaration.definition(DefinitionKind::Trait, name))
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown native trait"))
    }
}
