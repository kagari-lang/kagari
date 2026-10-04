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
use kagari_types::{
    callable::CallableImplementation,
    declaration::{TypeDefKind, module::ModuleDecl},
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
        let owners = providers.receiver_owners(Some(&declaration))?;
        declaration
            .validate(&|receiver| owners.owner(receiver))
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
        let storage_types: Vec<_> = owned
            .types
            .iter()
            .filter(|(_, ty)| matches!(ty.kind, TypeDefKind::NativeStorage(_)))
            .collect();
        if storage_types.len() != storage.len()
            || storage_types.iter().any(|(id, ty)| {
                storage
                    .get(id)
                    .is_none_or(|storage| ty.kind != TypeDefKind::NativeStorage(storage.layout()))
            })
        {
            return Err(RuntimeError::metadata_conflict(
                "native storage entries differ from their declared types",
            ));
        }
        let mut available = owned.clone();
        available.merge(providers)?;
        for target in declaration.exports.values() {
            let mut owner = target.clone();
            owner.path.truncate(1);
            let exists = match target.path[0].kind {
                DefinitionKind::Trait => available.traits.get(target).is_some(),
                DefinitionKind::Enum | DefinitionKind::AssociatedType => {
                    available.types.get(&owner).is_some_and(|ty| {
                        target.path.len() == 1
                            || ty
                                .variants
                                .iter()
                                .any(|variant| variant.name == target.path[1].name)
                    })
                }
                _ => false,
            };
            if !exists {
                return Err(RuntimeError::metadata_conflict(
                    "re-export target is not declared by an available provider",
                ));
            }
        }
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
        let mut dependencies = dependencies;
        for target in declaration.exports.values() {
            let mut owner = target.clone();
            owner.path.truncate(1);
            match target.path[0].kind {
                DefinitionKind::Trait => {
                    dependencies.insert(
                        target.clone(),
                        available.paths(available.traits.get(target).expect("checked export"))?,
                    )?;
                }
                _ => {
                    dependencies.insert_type(
                        owner.clone(),
                        available.paths(available.types.get(&owner).expect("checked export"))?,
                    )?;
                }
            }
        }
        let required = dependencies.foreign_to(&owned)?;
        let mut declaration = declaration;
        declaration.dependencies.extend(
            required
                .traits
                .keys()
                .chain(required.types.keys())
                .chain(required.declarations.keys())
                .chain(required.implementations.keys())
                .map(|id| id.module.clone())
                .filter(|owner| *owner != declaration.identity)
                .collect::<Vec<_>>(),
        );
        declaration
            .validate(&|receiver| owners.owner(receiver))
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

    /// Publish a mutually dependent foundation as one validated installation.
    pub fn install_all(modules: &[Self], runtime: &mut Runtime) -> NativeResult<()> {
        let mut staged = runtime.native_entries.clone();
        for module in modules {
            module.publish_owners(&mut staged)?;
        }
        for module in modules {
            module.publish_bindings(&mut staged)?;
        }
        staged.catalog.validate_callable_contracts()?;
        runtime.native_entries = staged;
        Ok(())
    }

    pub(crate) fn install_into(&self, registry: &mut NativeRegistry) -> NativeResult<()> {
        let mut staged = registry.clone();
        self.publish_owners(&mut staged)?;
        self.publish_bindings(&mut staged)?;
        staged.catalog.validate_callable_contracts()?;
        *registry = staged;
        Ok(())
    }

    fn publish_owners(&self, registry: &mut NativeRegistry) -> NativeResult<()> {
        for id in self.owned.traits.keys() {
            if registry.catalog.get(&id).is_some() {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native trait owner",
                ));
            }
        }
        for id in self.owned.types.keys() {
            if registry.catalog.types.get(&id).is_some() {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native type owner",
                ));
            }
        }
        for (id, storage) in self.storage.iter() {
            if registry
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
        registry.catalog.merge(&self.owned)?;
        Ok(())
    }

    fn publish_bindings(&self, registry: &mut NativeRegistry) -> NativeResult<()> {
        if !self.required.satisfied_by(&registry.catalog)? {
            return Err(RuntimeError::metadata_conflict(
                "native module dependency is not installed",
            ));
        }
        registry
            .catalog
            .check_implementations([&self.to_declaration()?])?;
        let mut imports = CatalogImports::new(registry.catalog.types.context().clone());
        for registration in self.bindings.iter() {
            registry.install(registration.clone(), &mut imports)?;
        }
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
