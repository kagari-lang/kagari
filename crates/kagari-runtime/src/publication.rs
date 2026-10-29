//! Host-boundary publication admission, independent of parked execution storage.
use crate::{
    Runtime, StagedReload,
    error::RuntimeError,
    module::{Program, VerifiedProgram},
    reload::ReloadValidationError,
    resource::ResourceState,
};
use std::{cell::Cell, sync::Arc};

#[derive(Debug, Default)]
pub(crate) struct PublicationState {
    synchronous: Cell<usize>,
    publishing: Cell<bool>,
}

/// A synchronous entry or driver scope, not a retained session or frame.
/// Keep it alive through callbacks and cleanup, even when the active root is hidden.
pub(crate) struct SynchronousActivation<'runtime> {
    resources: &'runtime ResourceState,
}

impl ResourceState {
    pub(crate) fn publication_active(&self) -> bool {
        self.publication.publishing.get()
    }

    pub(crate) fn ensure_not_publishing(&self) -> Result<(), RuntimeError> {
        if self.publication.publishing.get() {
            return Err(RuntimeError::execution_phase_violation(
                "execution or driver entry during publication",
            ));
        }
        Ok(())
    }

    // Liveness is checked by entry owners. Cleanup also needs this scope after
    // quarantine/close, without reviving execution or weakening teardown.
    pub(crate) fn begin_synchronous_activation(
        &self,
    ) -> Result<SynchronousActivation<'_>, RuntimeError> {
        self.ensure_not_publishing()?;
        let next = self
            .publication
            .synchronous
            .get()
            .checked_add(1)
            .ok_or_else(|| self.quarantine("synchronous activation count overflow"))?;
        self.publication.synchronous.set(next);
        Ok(SynchronousActivation { resources: self })
    }
}

impl Drop for SynchronousActivation<'_> {
    fn drop(&mut self) {
        let count = &self.resources.publication.synchronous;
        if let Some(next) = count.get().checked_sub(1) {
            count.set(next);
        } else {
            self.resources
                .quarantine("synchronous activation count underflow");
        }
    }
}

/// Exclusive publication authority for one live installation in this Runtime.
///
/// Acquire before source preparation for ordinary updates. Metadata-only staging
/// remains available while held, but script entry, resumption, driver consumption
/// and another publication are rejected. Parked executions and producer-side
/// completion notifications are unaffected. Dropping the guard releases admission
/// on success, error or unwinding; it does not publish an abandoned candidate.
///
/// This borrows the exclusively driven Runtime rather than owning it. Exclusivity
/// is runtime-wide and dynamically checked, including reentry through `&Runtime`.
#[must_use]
pub struct PublicationGuard<'runtime> {
    runtime: &'runtime Runtime,
    target: Program,
}

impl Runtime {
    /// Admit a host-top-level update before preparation or candidate effects.
    ///
    /// Rejects synchronous execution/driver scopes and another publication with
    /// `ExecutionPhaseViolation`. Foreign/retired targets, native borrows and
    /// closed/faulted runtimes retain their existing error categories.
    pub fn begin_publication(
        &self,
        target: &Program,
    ) -> Result<PublicationGuard<'_>, RuntimeError> {
        self.resources().ensure_not_publishing()?;
        self.check_publication_boundary()?;
        self.select_program(target)?;
        self.resources().publication.publishing.set(true);
        Ok(PublicationGuard {
            runtime: self,
            target: target.clone(),
        })
    }

    /// Initial installation uses an exclusive Runtime borrow instead of exposing
    /// a live target handle before commit. Forgotten scopes still fail admission.
    pub(crate) fn check_initial_publication(&self) -> Result<(), RuntimeError> {
        self.resources().ensure_not_publishing()?;
        self.check_publication_boundary()
    }

    fn check_publication_boundary(&self) -> Result<(), RuntimeError> {
        if self.resources().publication.synchronous.get() != 0 {
            return Err(RuntimeError::execution_phase_violation(
                "publication requires an idle synchronous execution boundary",
            ));
        }
        self.resources().ensure_execution_allowed()?;
        self.ensure_scheduler_open()?;
        self.gc().ensure_no_storage_borrow()
    }
}

impl PublicationGuard<'_> {
    /// Stage verified executable metadata for this guard's installation.
    /// No script, initializer or host callback is invoked by linking.
    pub fn stage_reload_verified_program(
        &self,
        program_name: Option<String>,
        program: VerifiedProgram,
    ) -> Result<StagedReload, ReloadValidationError> {
        self.recheck().map_err(ReloadValidationError::Runtime)?;
        self.runtime
            .stage_reload_verified_program(&self.target, program_name, program)
    }

    /// Publish a staged candidate, including one prepared before admission.
    /// Rechecks runtime admission and target/baseline authority before commit.
    /// Consumes this guard without acquiring a second publication guard.
    pub fn publish(self, candidate: StagedReload) -> Result<(), ReloadValidationError> {
        if !Arc::ptr_eq(&self.target.installation, &candidate.target.installation) {
            return Err(ReloadValidationError::Runtime(
                RuntimeError::module_validation("candidate belongs to another publication target"),
            ));
        }
        self.runtime.publish_admitted_reload(candidate, &self)
    }

    pub(crate) fn recheck(&self) -> Result<(), RuntimeError> {
        self.runtime.check_publication_boundary()?;
        self.runtime.select_program(&self.target)?;
        Ok(())
    }
}

impl Drop for PublicationGuard<'_> {
    fn drop(&mut self) {
        self.runtime.resources().publication.publishing.set(false);
    }
}
