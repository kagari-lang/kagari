//! Checked declarations and local Rust entries form one installable module.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        catalog::{DeclarationCatalog, import::CatalogImports},
        registry::{BindingRegistration, NativeRegistry},
        storage::NativeStorage,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionMap,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{TypeDefKind, module::ModuleDecl},
};
use std::{collections::BTreeMap, iter, slice, sync::Arc};

#[derive(Debug, Clone)]
pub struct NativeModule {
    declaration: Arc<DefinitionMetadata<ModuleDecl<DefinitionId>>>,
    bindings: Arc<[Arc<BindingRegistration>]>,
    owned: DeclarationCatalog,
    catalog: DeclarationCatalog,
    required: DeclarationCatalog,
    storage: Arc<DefinitionMap<NativeStorage>>,
}

impl NativeModule {
    pub(crate) fn checked(
        declaration: ModuleDecl,
        bindings: Vec<(DefinitionId, NativeBinding)>,
        storage: BTreeMap<DefinitionId, NativeStorage>,
        providers: &DeclarationCatalog,
    ) -> NativeResult<Self> {
        let context = providers.types.context();
        let definitions = declaration
            .collect_definitions(context)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        let owners = providers.receiver_owners(Some(&declaration))?;
        declaration
            .validate(
                &|receiver| owners.owner(receiver, &definitions),
                &definitions,
            )
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        let owned = DeclarationCatalog::declared([&declaration], context)?;
        let mut entries = BTreeMap::new();
        for declaration in owned.declarations.values() {
            let CallableImplementation::Native(binding) = declaration.function.implementation
            else {
                return Err(RuntimeError::metadata_conflict(
                    "native method has no Rust binding",
                ));
            };
            entries
                .entry(binding)
                .or_insert_with(Vec::new)
                .push(declaration.clone());
        }
        let storage_types: Vec<_> = owned
            .types
            .entries()
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
        let definitions = available.definitions();
        for target in declaration.exports.values() {
            let owner = export_owner(*target, &definitions)?;
            let view = definitions
                .resolve(owner)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
            let exists = match view.last().map(|part| part.kind) {
                Some(DefinitionKind::Trait) => available.traits.get_id(*target).is_some(),
                Some(DefinitionKind::Enum | DefinitionKind::AssociatedType) => {
                    available.types.get_id(owner).is_some_and(|ty| {
                        owner == *target
                            || definitions
                                .resolve(*target)
                                .ok()
                                .and_then(|view| view.last())
                                .is_some_and(|member| {
                                    ty.variants
                                        .iter()
                                        .any(|variant| variant.name == member.name)
                                })
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
            available.dependency_closure(owned.traits.ids(), iter::empty(), [&declaration])?;
        let dependencies = available.binding_dependencies(&base, entries.values().flatten())?;
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
            let required_catalog = checked.binding_dependencies(&base, &declarations)?;
            let registration = Arc::new(BindingRegistration::checked(
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
            let owner = export_owner(*target, &definitions)?;
            if let Some(contract) = available.traits.get_id(*target) {
                dependencies.insert(*target, contract.clone())?;
            } else {
                dependencies.insert_type(
                    owner,
                    available
                        .types
                        .get_id(owner)
                        .expect("checked export")
                        .clone(),
                )?;
            }
        }
        let required = dependencies.foreign_to(&owned)?;
        let definitions = checked.definitions();
        let mut declaration = declaration;
        declaration.dependencies.extend(
            required
                .traits
                .ids()
                .chain(required.types.ids())
                .chain(required.declarations.ids())
                .chain(required.implementations.ids())
                .map(|id| {
                    definitions
                        .resolve(id)
                        .map(|view| view.module().clone())
                        .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|owner| *owner != declaration.identity),
        );
        declaration
            .validate(
                &|receiver| owners.owner(receiver, &definitions),
                &definitions,
            )
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        let mut indexed_storage = DefinitionMap::new(owned.types.context().clone());
        for (id, storage) in storage {
            indexed_storage
                .insert_id(id, storage)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        let declaration = DefinitionMetadata::checked(
            checked.definitions(),
            declaration,
            &CancellationToken::default(),
        )
        .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))?;
        Ok(Self {
            declaration: Arc::new(declaration),
            bindings: registrations.into(),
            owned,
            catalog: checked,
            required,
            storage: Arc::new(indexed_storage),
        })
    }

    pub fn declaration(&self) -> &ModuleDecl<DefinitionId> {
        self.declaration.records()
    }

    pub fn definitions(&self) -> &DefinitionTable {
        self.declaration.definitions()
    }

    /// Extract editable declaration records while retaining their identity scope.
    /// This does not transfer installation or binding verification evidence.
    pub fn to_declaration(&self) -> DefinitionMetadata<ModuleDecl> {
        self.declaration.as_ref().clone()
    }

    pub fn catalog(&self) -> DeclarationCatalog {
        self.catalog.clone()
    }

    pub fn install(&self, runtime: &mut Runtime) -> NativeResult<()> {
        self.install_into(&mut runtime.native_entries)
    }

    /// Publish a mutually dependent foundation as one validated installation.
    pub fn install_all(modules: &[Self], runtime: &mut Runtime) -> NativeResult<()> {
        Self::install_batch(modules, &mut runtime.native_entries)
    }

    pub(crate) fn install_batch(
        modules: &[Self],
        registry: &mut NativeRegistry,
    ) -> NativeResult<()> {
        let mut staged = registry.clone();
        let mut imports = CatalogImports::new(staged.catalog.types.context().clone());
        for module in modules {
            module.publish_owners(&mut staged, &mut imports)?;
        }
        for module in modules {
            module.publish_bindings(&mut staged, &mut imports)?;
        }
        staged.catalog.validate_callable_contracts()?;
        *registry = staged;
        Ok(())
    }

    pub(crate) fn install_into(&self, registry: &mut NativeRegistry) -> NativeResult<()> {
        Self::install_batch(slice::from_ref(self), registry)
    }

    fn publish_owners(
        &self,
        registry: &mut NativeRegistry,
        imports: &mut CatalogImports,
    ) -> NativeResult<()> {
        let owned = imports.import(&self.owned)?;
        for id in owned.traits.ids() {
            if registry.catalog.traits.get_id(id).is_some() {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native trait owner",
                ));
            }
        }
        for id in owned.types.ids() {
            if registry.catalog.types.get_id(id).is_some() {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native type owner",
                ));
            }
        }
        let storage_ids = registry
            .storage
            .context()
            .import(&self.storage.context().snapshot(), self.storage.ids())
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        for (id, storage) in self.storage.entries() {
            if registry
                .storage
                .insert_id(
                    storage_ids
                        .map(id)
                        .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?,
                    storage.clone(),
                )
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
                .is_some()
            {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate native storage owner",
                ));
            }
        }
        registry.catalog.merge(&owned)?;
        Ok(())
    }

    fn publish_bindings(
        &self,
        registry: &mut NativeRegistry,
        imports: &mut CatalogImports,
    ) -> NativeResult<()> {
        if !self.required.satisfied_by(&registry.catalog)? {
            return Err(RuntimeError::metadata_conflict(
                "native module dependency is not installed",
            ));
        }
        let declaration = self
            .declaration
            .import_into(
                registry.catalog.types.context(),
                &CancellationToken::default(),
            )
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        declaration
            .records()
            .collect_definitions(registry.catalog.types.context())
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        registry
            .catalog
            .check_implementations([declaration.records()])?;
        for registration in self.bindings.iter() {
            registry.install(registration.clone(), imports)?;
        }
        Ok(())
    }

    pub fn trait_id(&self, name: &str) -> NativeResult<DefinitionId> {
        if !self
            .declaration()
            .traits
            .iter()
            .any(|contract| contract.name == name)
        {
            return Err(RuntimeError::metadata_conflict("unknown native trait"));
        }
        self.declaration()
            .definition(DefinitionKind::Trait, name, self.definitions())
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))
    }
}

fn export_owner(target: DefinitionId, definitions: &DefinitionTable) -> NativeResult<DefinitionId> {
    let view = definitions
        .resolve(target)
        .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
    if view
        .last()
        .is_some_and(|part| part.kind == DefinitionKind::Variant)
    {
        definitions
            .parent(target)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
            .ok_or_else(|| RuntimeError::metadata_conflict("export variant has no owner"))
    } else {
        Ok(target)
    }
}
