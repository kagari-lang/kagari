//! A native package owns declarations and implementation bindings together.
use crate::{
    Runtime,
    error::RuntimeError,
    native::{
        NativeContext, NativeInvocationState,
        catalog::NativeCatalog,
        factory::NativeFactory,
        registration::{NativeEntry, NativeRegistration, NativeRegistry},
    },
};
use kagari_abi::{
    callable::CallableImplementation,
    native_api::{NativeModule, render::NativeApiSource},
};
use kagari_common::identity::DefinitionId;
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    rc::Rc,
    sync::Arc,
};

pub struct NativeHandler {
    binding: DefinitionId,
    scratch_slots: usize,
    entry: Rc<NativeEntry>,
}
impl NativeHandler {
    pub(crate) fn from_factory(binding: DefinitionId, factory: NativeFactory) -> Self {
        Self {
            binding,
            scratch_slots: factory.scratch_slots,
            entry: factory.entry,
        }
    }
    pub fn new(
        binding: DefinitionId,
        scratch_slots: usize,
        entry: impl Fn(&mut NativeContext<'_>) -> Result<Box<dyn NativeInvocationState>, RuntimeError>
        + 'static,
    ) -> Self {
        Self {
            binding,
            scratch_slots,
            entry: Rc::new(entry),
        }
    }
}

#[derive(Clone)]
pub struct NativeApi {
    modules: Vec<Arc<NativeModule>>,
    registrations: Vec<NativeRegistration>,
    required_traits: NativeCatalog,
}
impl fmt::Debug for NativeApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeApi")
            .field("modules", &self.modules)
            .finish_non_exhaustive()
    }
}
impl NativeApi {
    /// Validate owned declarations and their implementation bindings.
    /// Composition and installation also validate foreign trait contracts against
    /// actual providers; a declaration catalog alone cannot install a dependency.
    pub fn new(
        modules: Vec<NativeModule>,
        handlers: Vec<NativeHandler>,
    ) -> Result<Self, RuntimeError> {
        let invalid =
            || RuntimeError::metadata_conflict("duplicate, missing or unknown native API binding");
        let mut identities = HashSet::new();
        let mut declarations = BTreeMap::new();
        for module in &modules {
            module
                .validate()
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
            if !identities.insert(&module.identity) {
                return Err(invalid());
            }
            for declaration in module.native_declarations() {
                let CallableImplementation::Native(binding) = &declaration.function.implementation
                else {
                    return Err(invalid());
                };
                declarations
                    .entry(binding.clone())
                    .or_insert_with(Vec::new)
                    .push(declaration);
            }
        }
        let mut registry = NativeRegistry::default();
        let mut registrations = vec![];
        for handler in handlers {
            let declared = declarations.remove(&handler.binding).ok_or_else(invalid)?;
            let registration = NativeRegistration {
                declarations: declared,
                scratch_slots: handler.scratch_slots,
                entry: handler.entry,
                required_traits: NativeCatalog::default(),
            };
            registry.install(registration.clone())?;
            registrations.push(registration);
        }
        if !declarations.is_empty() {
            return Err(invalid());
        }
        let declared_traits = NativeCatalog::declared(&modules)?;
        for registration in &mut registrations {
            registration.required_traits = declared_traits.clone();
        }
        Ok(Self {
            modules: modules.into_iter().map(Arc::new).collect(),
            registrations,
            required_traits: NativeCatalog::default(),
        })
    }
    pub fn modules(&self) -> &[Arc<NativeModule>] {
        &self.modules
    }
    /// Read owned contracts and retained authoring dependencies without installing
    /// their handlers. Composition must still supply the actual owning packages.
    pub fn catalog(&self) -> NativeCatalog {
        let mut catalog = NativeCatalog::declared(self.modules.iter().map(AsRef::as_ref))
            .expect("validated native trait declarations");
        catalog
            .merge(&self.required_traits)
            .expect("validated native trait dependencies");
        catalog
    }
    pub(crate) fn require_traits(&mut self, required: NativeCatalog) -> Result<(), RuntimeError> {
        let declared = NativeCatalog::declared(self.modules.iter().map(AsRef::as_ref))?;
        let mut checked = declared;
        checked.merge(&required)?;
        for registration in &mut self.registrations {
            registration.required_traits = checked.clone();
        }
        self.required_traits = required;
        Ok(())
    }
    /// Compose validated packages without constructing a runtime or executing factories.
    pub fn combine(packages: Vec<Self>) -> Result<Self, RuntimeError> {
        let mut registry = NativeRegistry::default();
        let mut identities = HashSet::new();
        let mut modules = vec![];
        let mut registrations = vec![];
        let mut required_traits = NativeCatalog::default();
        for package in packages {
            required_traits.merge(&package.required_traits)?;
            for module in package.modules {
                if !identities.insert(module.identity.clone()) {
                    return Err(RuntimeError::metadata_conflict("duplicate native module"));
                }
                modules.push(module);
            }
            for registration in package.registrations {
                registry.install(registration.clone())?;
                registrations.push(registration);
            }
        }
        let declared = NativeCatalog::declared(modules.iter().map(AsRef::as_ref))?;
        declared.check_implementations(modules.iter().map(AsRef::as_ref))?;
        if !required_traits.satisfied_by(&declared) {
            return Err(RuntimeError::metadata_conflict(
                "missing or changed native trait dependency",
            ));
        }
        Ok(Self {
            modules,
            registrations,
            required_traits,
        })
    }
    pub fn declaration_sources(&self) -> Vec<NativeApiSource> {
        self.modules
            .iter()
            .map(|module| {
                module
                    .declaration_source()
                    .expect("validated native API presentation")
            })
            .collect()
    }
    pub fn install(&self, runtime: &mut Runtime) -> Result<(), RuntimeError> {
        self.install_into(&mut runtime.native_entries)
    }
    pub(crate) fn install_into(&self, registry: &mut NativeRegistry) -> Result<(), RuntimeError> {
        // Stage the registry so a conflicting package cannot partially publish handlers.
        let mut staged = registry.clone();
        staged.install_traits(NativeCatalog::declared(
            self.modules.iter().map(AsRef::as_ref),
        )?)?;
        staged.require_traits(&self.required_traits)?;
        staged.check_implementations(self.modules.iter().map(AsRef::as_ref))?;
        for registration in &self.registrations {
            staged.install(registration.clone())?;
        }
        *registry = staged;
        Ok(())
    }
}
