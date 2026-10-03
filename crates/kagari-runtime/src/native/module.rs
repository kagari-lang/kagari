//! Checked declarations and local Rust entries form one installable module.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        catalog::{
            DeclarationCatalog,
            import::{CatalogImports, CatalogScopes},
        },
        registry::{BindingRegistration, NativeRegistry},
        storage::NativeStorage,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath,
        map::DefinitionMap,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::CallableImplementation, declaration::ModuleDecl, types::TypeDefKind,
};
use std::{collections::BTreeMap, iter, rc::Rc, sync::Arc};

#[derive(Debug, Clone)]
pub struct NativeModule {
    declaration: Arc<DefinitionMetadata<ModuleDecl<DefinitionId>>>,
    bindings: Rc<[Rc<BindingRegistration>]>,
    owned: DeclarationCatalog,
    catalog: DeclarationCatalog,
    required: DeclarationCatalog,
    storage: Rc<DefinitionMap<NativeStorage>>,
}

impl NativeModule {
    pub(crate) fn checked(
        declaration: ModuleDecl,
        bindings: Vec<(DefinitionPath, NativeBinding)>,
        storage: BTreeMap<DefinitionPath, NativeStorage>,
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
                    .get(&id)
                    .is_none_or(|storage| ty.kind != TypeDefKind::NativeStorage(storage.layout()))
            })
        {
            return Err(RuntimeError::metadata_conflict(
                "native storage entries differ from their declared types",
            ));
        }
        let mut available = owned.clone();
        available.merge(providers)?;
        // Owned traits and implementation bounds are common to every binding.
        // A binding extends this closed seed only with its own signature/proofs.
        let base =
            available.dependency_closure(owned.traits.keys(), iter::empty(), [&declaration])?;
        let mut scopes = CatalogScopes::new(base.catalog.types.context().clone());
        let dependencies =
            available.binding_dependencies(&base, &mut scopes, entries.values().flatten())?;
        let mut checked = owned.clone();
        checked.merge(&dependencies)?;
        checked.check_implementations([&declaration])?;
        checked.validate_callable_contracts()?;
        let mut registrations = Vec::new();
        let mut registry = NativeRegistry::default();
        let mut imports = CatalogImports::new(registry.catalog.types.context().clone());
        for (id, binding) in bindings {
            let declarations = entries.remove(&id).ok_or_else(|| {
                RuntimeError::metadata_conflict("unknown or duplicate native binding")
            })?;
            let required_catalog =
                checked.binding_dependencies(&base, &mut scopes, &declarations)?;
            let registration = Rc::new(BindingRegistration::checked(
                declarations,
                binding,
                required_catalog,
            )?);
            registry.install(registration.clone(), &mut imports)?;
            registrations.push(registration);
        }
        if !entries.is_empty() {
            return Err(RuntimeError::metadata_conflict(
                "missing native implementation binding",
            ));
        }
        let required = dependencies.foreign_to(&owned)?;
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
        let mut indexed_storage = DefinitionMap::new(owned.types.context().clone());
        for (id, storage) in storage {
            indexed_storage
                .insert(id, storage)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        let records = checked.scope(&declaration)?;
        let declaration = DefinitionMetadata::checked(
            checked.definitions(),
            records,
            &CancellationToken::default(),
        )
        .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))?;
        Ok(Self {
            declaration: Arc::new(declaration),
            bindings: registrations.into(),
            owned,
            catalog: checked,
            required,
            storage: Rc::new(indexed_storage),
        })
    }

    pub fn declaration(&self) -> &ModuleDecl<DefinitionId> {
        self.declaration.records()
    }

    pub fn definitions(&self) -> &DefinitionTable {
        self.declaration.definitions()
    }

    /// Explicit source-authoring projection; installed modules retain short IDs.
    pub fn to_declaration(&self) -> NativeResult<ModuleDecl> {
        self.declaration
            .to_paths(&CancellationToken::default())
            .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))
    }

    pub fn catalog(&self) -> DeclarationCatalog {
        self.catalog.clone()
    }

    pub fn install(&self, runtime: &mut Runtime) -> NativeResult<()> {
        self.install_into(&mut runtime.native_entries)
    }

    pub(crate) fn install_into(&self, registry: &mut NativeRegistry) -> NativeResult<()> {
        let mut staged = registry.clone();
        if !self.required.satisfied_by(&staged.catalog)? {
            return Err(RuntimeError::metadata_conflict(
                "native module dependency is not installed",
            ));
        }
        let owned = &self.owned;
        for id in owned.traits.keys() {
            if staged.catalog.get(&id).is_some() {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native trait owner",
                ));
            }
        }
        for (id, storage) in self.storage.iter() {
            if staged
                .storage
                .insert(id, storage.clone())
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
                .is_some()
            {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native storage owner",
                ));
            }
        }
        staged.catalog.merge(owned)?;
        staged
            .catalog
            .check_implementations([&self.to_declaration()?])?;
        let mut imports = CatalogImports::new(staged.catalog.types.context().clone());
        for registration in self.bindings.iter() {
            staged.install(registration.clone(), &mut imports)?;
        }
        staged.catalog.validate_callable_contracts()?;
        *registry = staged;
        Ok(())
    }

    pub fn trait_id(&self, name: &str) -> NativeResult<DefinitionPath> {
        self.declaration()
            .traits
            .iter()
            .any(|contract| contract.name == name)
            .then(|| {
                ModuleDecl::new(self.declaration().identity.clone())
                    .definition(DefinitionKind::Trait, name)
            })
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown native trait"))
    }
}
