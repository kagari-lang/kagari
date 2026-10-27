//! Candidate leases invalidate access without owning or borrowing module storage.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    module::{ExecutableModule, Installation, ModuleKey, ModuleStore},
};
use std::{
    collections::hash_map::Entry,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

/// The store keeps only a Weak identity. Last release requests a collection
/// without borrowing storage or keeping the runtime/program graph alive.
#[derive(Debug)]
pub(super) struct CandidateLease {
    abandoned: Arc<AtomicBool>,
    published: AtomicBool,
    pub(super) initial_load: bool,
}

impl CandidateLease {
    pub(super) fn new(abandoned: Arc<AtomicBool>, initial_load: bool) -> Self {
        Self {
            abandoned,
            published: AtomicBool::new(false),
            initial_load,
        }
    }
}

impl Drop for CandidateLease {
    fn drop(&mut self) {
        if !self.published.load(Ordering::Relaxed) {
            self.abandoned.store(true, Ordering::Release);
        }
    }
}

/// Authorizes an unpublished program without owning its mutable storage.
/// Last-drop invalidates candidate access; collection retires its physical records.
#[derive(Debug)]
pub(crate) struct StagedProgram {
    pub(super) installation: Arc<Installation>,
    pub(super) module: ExecutableModule,
    pub(super) lease: Arc<CandidateLease>,
    pub(super) expected_base: Option<ModuleKey>,
}

impl StagedProgram {
    pub(crate) fn module(&self) -> &ExecutableModule {
        &self.module
    }

    pub(crate) fn bind_reload(
        self,
        store: &ModuleStore,
        baseline: &ExecutableModule,
    ) -> Result<Self, RuntimeError> {
        if self.expected_base != Some(baseline.key())
            || self.module.installation_id() != baseline.installation_id()
        {
            return Err(RuntimeError::module_validation(
                "reload demand history belongs to a different baseline",
            ));
        }
        store.bind_static_reload(baseline, &self.module)?;
        Ok(self)
    }

    pub(crate) fn publish(self, store: &ModuleStore) -> Result<ExecutableModule, RuntimeError> {
        store.validate_installation(&self.installation, self.module.registry_owner)?;
        store.publish_program(&self.module)?;
        let replacement = self.module.program.clone();
        let mut inner = store.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed during publication")
        })?;
        let key = self.module.program_key();
        let valid = inner.resolve(&self.module).is_some()
            && inner
                .staged
                .get(&key)
                .is_some_and(|lease| Weak::ptr_eq(lease, &Arc::downgrade(&self.lease)));
        if !valid || self.module.installation_id() != self.installation.id {
            return Err(RuntimeError::module_validation(
                "invalid staged program publication",
            ));
        }
        // Resolve the hash-table entry before taking the cell lock. Removing this
        // occupied entry in the commit neither allocates nor hashes again.
        let entry = match inner.staged.entry(key) {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(_) => {
                return Err(RuntimeError::module_validation(
                    "missing staged publication",
                ));
            }
        };
        let mut current = self.installation.current.lock().map_err(|_| {
            RuntimeError::new(
                RuntimeErrorKind::EngineFault,
                "installation metadata lock poisoned",
            )
        })?;
        let current_key = current.as_ref().map(|program| {
            let root = &program.modules[program.root.index()];
            ModuleKey {
                id: root.id,
                epoch: root.epoch,
            }
        });
        if current_key != self.expected_base {
            return Err(RuntimeError::new(
                RuntimeErrorKind::StaleHandle,
                "installation publication baseline changed or closed",
            ));
        }
        // Everything below is non-failing; no destruction of the old descriptor
        // occurs until both guards have been released.
        self.module.program.dispatch.scope.mark_published();
        self.lease.published.store(true, Ordering::Relaxed);
        let removed_stage = entry.remove();
        let previous = current.replace(replacement);
        drop(current);
        drop(inner);
        drop(previous);
        drop(removed_stage);
        Ok(self.module.clone())
    }
}

#[cfg(test)]
mod tests;
