//! Host-boundary publication admission, independent of parked execution storage.
use crate::{
    Runtime, StagedReload,
    error::{RuntimeError, fault::EngineFault},
    module::{InstalledProgram, VerifiedProgram},
    reload::ReloadValidationError,
    resource::ResourceState,
};

use std::{
    cell::Cell,
    marker::PhantomData,
    rc::Rc,
    sync::Arc,
    thread::{self, ThreadId},
};

#[derive(Debug, Default)]
pub(crate) struct PublicationState {
    synchronous: Cell<usize>,
    thread: Cell<Option<ThreadId>>,
    publishing: Cell<bool>,
}

/// A synchronous entry or driver scope, not a retained session or frame.
/// Keep it alive through callbacks and cleanup, even when the active root is hidden.
pub(crate) struct SynchronousActivation<'runtime> {
    resources: &'runtime ResourceState,
    active: bool,
}

/// Lifetime-free admission owned by a session's cleanup record. Construction
/// keeps the borrowing guard until admission succeeds, then transfers it once.
#[must_use]
#[derive(Default)]
pub(crate) struct ActivationMark(PhantomData<Rc<()>>);

impl ActivationMark {
    pub(crate) fn attach(self, resources: &ResourceState) -> SynchronousActivation<'_> {
        SynchronousActivation {
            resources,
            active: true,
        }
    }

    pub(crate) fn retire(self, resources: &ResourceState) {
        if resources.ensure_synchronous_thread().is_err() {
            resources.quarantine(EngineFault::SynchronousActivationEndedOnAnotherThread);
            return;
        }
        let count = &resources.publication.synchronous;
        if let Some(next) = count.get().checked_sub(1) {
            count.set(next);
            if next == 0 {
                resources.publication.thread.set(None);
            }
        } else {
            resources.quarantine(EngineFault::SynchronousActivationCountUnderflow);
        }
    }
}

impl SynchronousActivation<'_> {
    pub(crate) fn detach(mut self) -> ActivationMark {
        self.active = false;
        ActivationMark::default()
    }
}

impl ResourceState {
    pub(crate) fn ensure_synchronous_thread(&self) -> Result<(), RuntimeError> {
        if self
            .publication
            .thread
            .get()
            .is_some_and(|owner| owner != thread::current().id())
        {
            return Err(RuntimeError::execution_phase_violation(
                "active Runtime services belong to another thread",
            ));
        }
        Ok(())
    }

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
        self.ensure_synchronous_thread()?;
        let next = self
            .publication
            .synchronous
            .get()
            .checked_add(1)
            .ok_or_else(|| self.quarantine(EngineFault::SynchronousActivationCountOverflow))?;
        if next == 1 {
            self.publication.thread.set(Some(thread::current().id()));
        }
        self.publication.synchronous.set(next);
        Ok(SynchronousActivation {
            resources: self,
            active: true,
        })
    }
}

impl Drop for SynchronousActivation<'_> {
    fn drop(&mut self) {
        if self.active {
            ActivationMark::default().retire(self.resources);
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
    runtime: &'runtime mut Runtime,
    permit: PublicationPermit,
}

pub(crate) struct PublicationPermit {
    target: InstalledProgram,
}

impl PublicationPermit {
    pub(crate) fn recheck(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        runtime.check_publication_boundary()?;
        runtime.select_program(&self.target)?;
        Ok(())
    }
}

impl Runtime {
    /// Admit a host-top-level update before preparation or candidate effects.
    ///
    /// Rejects synchronous execution/driver scopes and another publication with
    /// `ExecutionPhaseViolation`. Foreign/retired targets and closed/faulted
    /// runtimes retain their existing error categories. Scoped payload reads
    /// must end before borrowing Runtime for publication.
    pub fn begin_publication(
        &mut self,
        target: &InstalledProgram,
    ) -> Result<PublicationGuard<'_>, RuntimeError> {
        self.resources().ensure_not_publishing()?;
        self.check_publication_boundary()?;
        self.select_program(target)?;
        self.resources().publication.publishing.set(true);
        Ok(PublicationGuard {
            runtime: self,
            permit: PublicationPermit {
                target: target.clone(),
            },
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
        self.resources().ensure_synchronous_thread()
    }
}

impl PublicationGuard<'_> {
    /// Read-only Runtime access while publication authority is held. Execution,
    /// driving and nested publication remain dynamically forbidden.
    pub fn runtime(&self) -> &Runtime {
        self.runtime
    }

    /// Stage verified executable metadata for this guard's installation.
    /// No script, initializer or host callback is invoked by linking.
    pub fn stage_reload_verified_program(
        &mut self,
        program_name: Option<String>,
        program: VerifiedProgram,
    ) -> Result<StagedReload, ReloadValidationError> {
        self.permit
            .recheck(self.runtime)
            .map_err(ReloadValidationError::Runtime)?;
        self.runtime
            .stage_reload_verified_program(&self.permit.target, program_name, program)
    }

    /// Publish a staged candidate, including one prepared before admission.
    /// Rechecks runtime admission and target/baseline authority before commit.
    /// Consumes this guard without acquiring a second publication guard.
    pub fn publish(self, candidate: StagedReload) -> Result<(), ReloadValidationError> {
        if !Arc::ptr_eq(
            &self.permit.target.installation,
            &candidate.target.installation,
        ) {
            return Err(ReloadValidationError::Runtime(
                RuntimeError::module_validation("candidate belongs to another publication target"),
            ));
        }
        self.runtime
            .publish_admitted_reload(candidate, &self.permit)
    }
}

impl Drop for PublicationGuard<'_> {
    fn drop(&mut self) {
        self.runtime.resources().publication.publishing.set(false);
    }
}

#[cfg(test)]
mod tests;
