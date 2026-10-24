//! Installation identity and live metadata admission, separate from executable leases.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    host::HostRegistryId,
    module::{
        ExecutableModule, Installation, InstallationId, Module, ModuleKey, ModuleSnapshot,
        ModuleStore, Program,
    },
};
use kagari_common::identity::ModuleIdentity;
use std::sync::{Arc, Mutex, atomic::Ordering};

impl Program {
    /// Construct the root namespace identity, even after the installation is closed.
    /// Lookup and binding through that identity still require a live installation.
    pub fn root(&self) -> Module {
        self.installation
            .namespace_handle(self.installation.root_identity.clone())
    }

    pub(crate) fn capture_current(&self) -> Result<ExecutableModule, RuntimeError> {
        self.installation.capture_current()
    }
}

impl Module {
    pub(crate) fn capture_current(&self) -> Result<ModuleSnapshot, RuntimeError> {
        Ok(ModuleSnapshot {
            root: self.installation.capture_current()?,
            namespace: self.namespace.clone(),
        })
    }

    pub(crate) fn namespace_handle(&self, namespace: ModuleIdentity) -> Self {
        self.installation.namespace_handle(namespace)
    }
}

impl Installation {
    pub(super) fn reserve_declaration(&self) -> Result<u64, RuntimeError> {
        self.next_declaration
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map_err(|_| RuntimeError::resource_limit("stable declaration identity exhausted"))
    }

    pub(crate) fn namespace_handle(self: &Arc<Self>, namespace: ModuleIdentity) -> Module {
        Module {
            installation: self.clone(),
            namespace,
        }
    }

    pub(crate) fn capture_current(&self) -> Result<ExecutableModule, RuntimeError> {
        let program = self
            .current
            .lock()
            .map_err(|_| {
                RuntimeError::new(
                    RuntimeErrorKind::EngineFault,
                    "installation metadata lock poisoned",
                )
            })?
            .clone()
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::StaleHandle, "installation is closed")
            })?;
        Ok(ExecutableModule {
            slot: program.root,
            program,
        })
    }

    fn retire(&self) {
        // Poison recovery may revoke authority, never admit metadata or execution.
        let previous = match self.current.lock() {
            Ok(mut current) => current.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        drop(previous);
    }
}

impl ModuleStore {
    pub(crate) fn reserve_installation(
        &mut self,
        registry_owner: HostRegistryId,
        root_identity: ModuleIdentity,
    ) -> Result<Arc<Installation>, RuntimeError> {
        let inner = self.inner.get_mut();
        let id = InstallationId(inner.next_installation);
        inner.next_installation = inner
            .next_installation
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("installation identity space exhausted"))?;
        self.installations
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("installation registry"))?;
        let installation = Arc::new(Installation {
            registry_owner,
            id,
            root_identity,
            current: Mutex::new(None),
            next_declaration: Default::default(),
        });
        self.installations.insert(id, installation.clone());
        Ok(installation)
    }

    pub(crate) fn validate_installation(
        &self,
        installation: &Arc<Installation>,
        owner: HostRegistryId,
    ) -> Result<(), RuntimeError> {
        if installation.registry_owner != owner
            || !self
                .installations
                .get(&installation.id)
                .is_some_and(|registered| Arc::ptr_eq(registered, installation))
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::StaleHandle,
                "foreign or unregistered installation",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_executable_installation(
        &self,
        module: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        let installation = self
            .installations
            .get(&module.installation_id())
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::StaleHandle, "unregistered installation")
            })?;
        self.validate_installation(installation, module.registry_owner)?;
        let live = installation
            .current
            .lock()
            .map_err(|_| {
                RuntimeError::new(
                    RuntimeErrorKind::EngineFault,
                    "installation metadata lock poisoned",
                )
            })?
            .is_some();
        if live {
            return Ok(());
        }
        // Initial linking needs exact candidate metadata before the first
        // publication. StagedLoad retains this lease without exposing executable
        // entry authority; reload leases cannot authorize a retired installation.
        let inner = self.inner.try_borrow().map_err(|_| {
            RuntimeError::module_validation("module store borrowed during admission")
        })?;
        if inner
            .staged
            .get(&module.program_key())
            .and_then(|lease| lease.upgrade())
            .is_some_and(|lease| lease.initial_load)
        {
            Ok(())
        } else {
            Err(RuntimeError::new(
                RuntimeErrorKind::StaleHandle,
                "installation is closed",
            ))
        }
    }

    pub(crate) fn current_for(
        &self,
        module: &ExecutableModule,
    ) -> Result<ExecutableModule, RuntimeError> {
        let installation = self
            .installations
            .get(&module.installation_id())
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::StaleHandle, "unregistered installation")
            })?;
        self.validate_installation(installation, module.registry_owner)?;
        installation.capture_current()
    }

    pub(super) fn current_roots(&self) -> Result<Vec<ModuleKey>, RuntimeError> {
        self.installations
            .values()
            .filter_map(|installation| match installation.capture_current() {
                Ok(module) => Some(Ok(module.program_key())),
                Err(error) if error.kind() == RuntimeErrorKind::StaleHandle => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    /// Revoke all live metadata authority before Runtime state is released.
    /// This does not borrow executable storage and recovers poison only for clearing.
    pub(crate) fn retire_installations(&self) {
        for installation in self.installations.values() {
            installation.retire();
        }
    }
}

impl Runtime {
    /// Capture the current root of this Runtime's live installation once.
    /// The returned exact publication is not itself an executable retention lease.
    pub fn select_program(&self, program: &Program) -> Result<ExecutableModule, RuntimeError> {
        self.modules
            .validate_installation(&program.installation, self.host.owner())?;
        let root = program.capture_current()?;
        self.validate_executable_module(&root)?;
        Ok(root)
    }

    pub(crate) fn capture_module(&self, module: &Module) -> Result<ModuleSnapshot, RuntimeError> {
        self.modules
            .validate_installation(&module.installation, self.host.owner())?;
        let snapshot = module.capture_current()?;
        self.validate_executable_module(&snapshot.root)?;
        Ok(snapshot)
    }
}
