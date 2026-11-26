//! Checked declarations and local Rust entries form one installable module.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    host::HostPathAdapter,
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
        DefinitionKind, DefinitionPath,
        map::DefinitionMap,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{TraitDef, TypeDefKind, module::ModuleDecl, path::HostPathDeclaration},
    language::binding::future_declaration,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    iter, slice,
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct NativeModule {
    declaration: Arc<DefinitionMetadata<ModuleDecl>>,
    bindings: Arc<[Arc<BindingRegistration>]>,
    owned: DeclarationCatalog,
    catalog: DeclarationCatalog,
    installed_required: DeclarationCatalog,
    authored_required: Option<DeclarationCatalog>,
    required_source_traits: Arc<BTreeSet<DefinitionPath>>,
    // An explicit pin also constrains that owner if loaded without this importer.
    // Inferred uses constrain only programs containing the importing provider.
    explicit_owner_pins: Arc<BTreeSet<DefinitionPath>>,
    pub(super) authored_seed: Option<DeclarationCatalog>,
    storage: Arc<DefinitionMap<NativeStorage>>,
    path_bindings: Arc<DefinitionMap<Vec<HostPathAdapter>>>,
}

impl NativeModule {
    pub(crate) fn checked(
        declaration: ModuleDecl,
        bindings: Vec<(DefinitionId, NativeBinding)>,
        storage: BTreeMap<DefinitionId, NativeStorage>,
        path_bindings: Vec<(HostPathDeclaration, HostPathAdapter)>,
        explicit_owner_pins: BTreeSet<DefinitionPath>,
        providers: &DeclarationCatalog,
    ) -> NativeResult<Self> {
        let context = providers.types.context();
        declaration
            .collect_definitions(context)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        providers.validate_module_shape(&declaration)?;

        let mut owned = DeclarationCatalog::declared([&declaration], context)?;
        let mut path_bindings = path_bindings;
        let mut indexed_paths = DefinitionMap::new(context.clone());
        for (root, paths) in owned.paths.entries() {
            let mut adapters = Vec::with_capacity(paths.len());
            for path in paths {
                let Some(index) = path_bindings.iter().position(|(bound, _)| bound == path) else {
                    return Err(RuntimeError::metadata_conflict(
                        "missing manual path binding",
                    ));
                };
                let (_, adapter) = path_bindings.remove(index);
                adapter.check_access(path.access)?;
                adapters.push(adapter);
            }
            indexed_paths
                .insert_id(root, adapters)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        if !path_bindings.is_empty() {
            return Err(RuntimeError::metadata_conflict(
                "unknown or duplicate manual path binding",
            ));
        }
        owned.copy_rust_types(providers)?;
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
                !ty.fields.is_empty()
                    || storage.get(id).is_none_or(|storage| {
                        ty.kind != TypeDefKind::NativeStorage(storage.layout())
                    })
            })
        {
            return Err(RuntimeError::metadata_conflict(
                "native storage entries differ from their declared types",
            ));
        }
        let mut available = owned.clone();
        available.merge(providers)?;
        available.validate_paths()?;
        available.validate_module_declarations(&declaration)?;
        // Owned traits and implementation bounds are common to every binding.
        // A binding extends this closed seed only with its own signature/proofs.
        let mut required_source_traits = explicit_owner_pins.clone();

        let base =
            available.dependency_closure(owned.traits.ids(), iter::empty(), [&declaration])?;
        let dependencies = available.module_dependencies(
            &base,
            &declaration,
            &mut required_source_traits,
            entries.values().flatten(),
        )?;
        let mut checked = owned.clone();
        checked.merge(&dependencies)?;
        checked.copy_rust_types(&available)?;
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
            let owner = available.checked_export_owner(*target)?;
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
                .chain(required.source_impls.ids())
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
        available.validate_module_shape(&declaration)?;
        let authored_required = required.nonempty_authored_contracts();
        let installed_required = if let Some(authored) = &authored_required {
            required.foreign_to(authored)?
        } else {
            required
        };
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
            installed_required,
            authored_required,
            required_source_traits: Arc::new(required_source_traits),
            explicit_owner_pins: Arc::new(explicit_owner_pins),
            authored_seed: providers.nonempty_authored_contracts(),
            storage: Arc::new(indexed_storage),
            path_bindings: Arc::new(indexed_paths),
        })
    }

    pub fn declaration(&self) -> &ModuleDecl {
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

    /// Canonical trait paths that this importer's executable must carry, even without calls.
    /// Pass identities, not the installation's expected ABIs, to compiler contract emission.
    pub fn required_source_traits(&self) -> &BTreeSet<DefinitionPath> {
        &self.required_source_traits
    }

    /// Return the independently verified owner ABI for a mandatory importer path.
    ///
    /// The returned definition table scopes all identities in the ABI. Paths outside
    /// `required_source_traits()` are rejected, even if present in the authoring catalog.
    pub fn required_source_trait_contract(
        &self,
        path: &DefinitionPath,
    ) -> NativeResult<DefinitionMetadata<TraitDef>> {
        if !self.required_source_traits.contains(path) {
            return Err(RuntimeError::metadata_conflict(format!(
                "not a mandatory source trait for {}: {path:?}",
                self.declaration().identity
            )));
        }
        let required = self.authored_required.as_ref().ok_or_else(|| {
            RuntimeError::metadata_conflict("missing verified source trait requirements")
        })?;
        let (_, abi) = retained_source_trait(required, path)?;
        DefinitionMetadata::checked(
            required.definitions(),
            abi.clone(),
            &CancellationToken::default(),
        )
        .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))
    }

    pub fn catalog(&self) -> DeclarationCatalog {
        self.catalog.clone()
    }

    pub fn install(&self, runtime: &mut Runtime) -> NativeResult<()> {
        Self::install_all(slice::from_ref(self), runtime)
    }

    /// Publish a mutually dependent foundation as one validated installation.
    pub fn install_all(modules: &[Self], runtime: &mut Runtime) -> NativeResult<()> {
        runtime.check_initial_publication()?;
        let mut staged = runtime.native_entries.clone();
        staged.metadata = runtime.types.snapshot();
        Self::install_batch(modules, &mut staged)?;
        let host_types = runtime.host.prepare_native_registration(&staged)?;
        // No callbacks run during preparation. Publishing these owned values
        // together cannot leave entries installed without their reflected schema.
        runtime.types = staged.metadata.runtime_registry();
        runtime.native_entries = staged;
        host_types.publish(&mut runtime.host);
        Ok(())
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
        let contracts = staged.validation_contracts()?;
        for module in modules {
            module.publish_bindings(&mut staged, &mut imports, &contracts)?;
        }
        contracts.validate_callable_contracts()?;
        contracts.validate_paths()?;
        staged.metadata = staged.metadata.prepare(&contracts)?;
        *registry = staged;
        Ok(())
    }

    fn publish_owners(
        &self,
        registry: &mut NativeRegistry,
        imports: &mut CatalogImports,
    ) -> NativeResult<()> {
        if self.declaration().identity == future_declaration().module {
            return Err(RuntimeError::metadata_conflict(
                "core Future belongs to authored core source",
            ));
        }
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
        let mut expected = BTreeSet::new();
        if let Some(required) = &self.authored_required {
            let required = imports.import(required)?;
            for path in self.required_source_traits() {
                let (id, abi) = retained_source_trait(&required, path)?;
                if self.explicit_owner_pins.contains(path) {
                    registry.pin_source_trait(id, abi)?;
                }
                expected.insert(id);
            }
            registry.record_authored_requirements(&self.declaration().identity, required)?;
        }
        registry.owned_traits.extend(owned.traits.ids());
        let referenced = self
            .declaration
            .import_into(
                registry.catalog.types.context(),
                &CancellationToken::default(),
            )
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;

        let definitions = referenced
            .records()
            .collect_definitions(registry.catalog.types.context())
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        registry.install_scripts(referenced.records())?;
        let owner = definitions
            .lookup_root(&referenced.records().identity)
            .expect("collected native module root");
        if let Some(installed) = registry.required_source_traits.get_id(owner) {
            if installed != &expected {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting source trait reference owner",
                ));
            }
        } else {
            registry
                .required_source_traits
                .insert_id(owner, expected)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        let path_ids = registry
            .path_bindings
            .context()
            .import(
                &self.path_bindings.context().snapshot(),
                self.path_bindings.ids(),
            )
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        for (root, adapters) in self.path_bindings.entries() {
            if registry
                .path_bindings
                .insert_id(
                    path_ids
                        .map(root)
                        .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?,
                    adapters.clone(),
                )
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?
                .is_some()
            {
                return Err(RuntimeError::metadata_conflict(
                    "duplicate manual path owner",
                ));
            }
        }
        Ok(())
    }

    fn publish_bindings(
        &self,
        registry: &mut NativeRegistry,
        imports: &mut CatalogImports,
        contracts: &DeclarationCatalog,
    ) -> NativeResult<()> {
        if !self.installed_required.satisfied_by(&registry.catalog)? {
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
        contracts.check_implementations([declaration.records()])?;
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

fn retained_source_trait<'a>(
    required: &'a DeclarationCatalog,
    path: &DefinitionPath,
) -> NativeResult<(DefinitionId, &'a TraitDef)> {
    required
        .traits
        .context()
        .lookup(path)
        .and_then(|id| required.traits.get_id(id).map(|abi| (id, abi)))
        .ok_or_else(|| {
            RuntimeError::metadata_conflict(format!(
                "missing verified source trait contract: {path:?}"
            ))
        })
}
