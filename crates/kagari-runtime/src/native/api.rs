//! A native package owns declarations and implementation bindings together.
use crate::{
    Runtime, RuntimeError,
    native::{
        NativeContext, NativeEntry, NativeInvocationState, NativeRegistration, NativeRegistry,
    },
};
use kagari_abi::{
    callable::CallableImplementation,
    native_api::{NativeApiSource, NativeModule},
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
}
impl fmt::Debug for NativeApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeApi")
            .field("modules", &self.modules)
            .finish_non_exhaustive()
    }
}
impl NativeApi {
    /// Validate the entire package before making it available to engines or runtimes.
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
            };
            registry.install(registration.clone())?;
            registrations.push(registration);
        }
        if !declarations.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            modules: modules.into_iter().map(Arc::new).collect(),
            registrations,
        })
    }
    pub fn modules(&self) -> &[Arc<NativeModule>] {
        &self.modules
    }
    /// Compose validated packages without constructing a runtime or executing factories.
    pub fn combine(packages: Vec<Self>) -> Result<Self, RuntimeError> {
        let mut registry = NativeRegistry::default();
        let mut identities = HashSet::new();
        let mut modules = vec![];
        let mut registrations = vec![];
        for package in packages {
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
        Ok(Self {
            modules,
            registrations,
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
        for registration in &self.registrations {
            staged.install(registration.clone())?;
        }
        *registry = staged;
        Ok(())
    }
}
