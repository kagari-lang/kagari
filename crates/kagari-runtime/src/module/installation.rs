//! Identity-only handles; the supplying Runtime owns live selection and revocation.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    host::HostRegistryId,
    module::{
        ExecutableModule, Installation, InstallationId, InstalledProgram, Module, ModuleKey,
        ModuleSnapshot, ModuleStore,
    },
};
use kagari_common::identity::ModuleIdentity;
use std::{mem, sync::Arc};

impl InstalledProgram {
    /// Construct the root namespace identity, including after close. Live queries
    /// require the supplying Runtime; this handle owns no executable publication.
    pub fn root(&self) -> Module {
        self.installation
            .namespace_handle(self.installation.root_identity.clone())
    }
}

impl Module {
    pub(crate) fn namespace_handle(&self, namespace: ModuleIdentity) -> Self {
        self.installation.namespace_handle(namespace)
    }
}

impl Installation {
    pub(crate) fn namespace_handle(self: &Arc<Self>, namespace: ModuleIdentity) -> Module {
        Module {
            installation: self.clone(),
            namespace,
        }
    }
}

impl ModuleStore {
    /// An active session reuses its admitted installation until explicit global
    /// retirement. Publication itself neither revokes nor compares this authority.
    pub(crate) fn activation_is_live(&self) -> bool {
        !self.retired.get()
    }

    pub(super) fn reserve_declaration(&self) -> Result<u64, RuntimeError> {
        let next = self.next_declaration.get();
        self.next_declaration
            .set(next.checked_add(1).ok_or_else(|| {
                RuntimeError::resource_limit("stable declaration identity exhausted")
            })?);
        Ok(next)
    }

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
        });
        self.installations.insert(id, installation.clone());
        self.retired.set(false);
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

    pub(crate) fn capture_current(
        &self,
        installation: &Arc<Installation>,
    ) -> Result<ExecutableModule, RuntimeError> {
        self.validate_installation(installation, installation.registry_owner)?;
        let program = self
            .current
            .try_borrow()
            .map_err(|_| {
                RuntimeError::module_validation("current programs borrowed during lookup")
            })?
            .get(&installation.id)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::StaleHandle, "installation is closed")
            })?;
        Ok(ExecutableModule {
            slot: program.root,
            program,
        })
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
        if self
            .current
            .try_borrow()
            .map_err(|_| {
                RuntimeError::module_validation("current programs borrowed during admission")
            })?
            .contains_key(&installation.id)
        {
            return Ok(());
        }
        // Initial metadata linking is admitted only by its live candidate lease.
        // A reload lease cannot authorize a closed installation.
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
        self.capture_current(installation)
    }

    pub(super) fn current_roots(&self) -> Result<Vec<ModuleKey>, RuntimeError> {
        let current = self.current.try_borrow().map_err(|_| {
            RuntimeError::module_validation("current programs borrowed during collection")
        })?;
        Ok(current
            .values()
            .map(|program| {
                let root = &program.modules[program.root.index()];
                ModuleKey {
                    id: root.id,
                    epoch: root.epoch,
                }
            })
            .collect())
    }

    /// Revoke authority independently of executable storage. No current-table
    /// borrow or callback escapes this operation; destruction follows the borrow.
    pub(crate) fn retire_installations(&self) {
        self.retired.set(true);
        let previous = mem::take(&mut *self.current.borrow_mut());
        drop(previous);
    }
}

impl Runtime {
    /// Capture an immutable publication through its supplying Runtime. The
    /// descriptor itself grants no executable retention or revival authority.
    pub fn select_program(
        &self,
        program: &InstalledProgram,
    ) -> Result<ExecutableModule, RuntimeError> {
        self.modules
            .validate_installation(&program.installation, self.host.owner())?;
        let root = self.modules.capture_current(&program.installation)?;
        self.validate_executable_module(&root)?;
        Ok(root)
    }

    pub(crate) fn capture_module(&self, module: &Module) -> Result<ModuleSnapshot, RuntimeError> {
        self.modules
            .validate_installation(&module.installation, self.host.owner())?;
        let snapshot = ModuleSnapshot {
            root: self.modules.capture_current(&module.installation)?,
            namespace: module.namespace.clone(),
        };
        self.validate_executable_module(&snapshot.root)?;
        Ok(snapshot)
    }
}
