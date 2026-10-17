//! Candidate leases invalidate access without owning or borrowing module storage.
use crate::{
    error::RuntimeError,
    module::{LoadedModule, ModuleStore},
};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

/// The store keeps only a Weak identity. Last release requests a collection
/// without borrowing storage or keeping the runtime/program graph alive.
#[derive(Debug)]
pub(super) struct CandidateLease {
    abandoned: Arc<AtomicBool>,
    published: AtomicBool,
}

impl CandidateLease {
    pub(super) fn new(abandoned: Arc<AtomicBool>) -> Self {
        Self {
            abandoned,
            published: AtomicBool::new(false),
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
    pub(super) module: LoadedModule,
    pub(super) lease: Arc<CandidateLease>,
}

impl StagedProgram {
    pub(crate) fn module(&self) -> &LoadedModule {
        &self.module
    }

    pub(crate) fn publish(self, store: &ModuleStore) -> Result<LoadedModule, RuntimeError> {
        let mut inner = store.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed during publication")
        })?;
        let key = self.module.program_key();
        let valid = inner.resolve(&self.module).is_some()
            && inner
                .staged
                .get(&key)
                .is_some_and(|lease| Weak::ptr_eq(lease, &Arc::downgrade(&self.lease)));
        if !valid {
            return Err(RuntimeError::module_validation(
                "invalid staged program publication",
            ));
        }
        self.lease.published.store(true, Ordering::Relaxed);
        inner.staged.remove(&key);
        inner
            .latest_by_name
            .insert(self.module.name.clone(), self.module.key());
        Ok(self.module.clone())
    }
}

#[cfg(test)]
mod tests;
