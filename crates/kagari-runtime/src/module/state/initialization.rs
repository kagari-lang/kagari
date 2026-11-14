//! Initializer authority uses actual caller sources and detached completion facts.
mod cleanup;
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::ExecutionRoots,
    module::{
        managed::{cell::StaticCellObject, source::RevisionSource},
        state::{
            StaticFailure, StaticRecipe,
            access::{Observation, StaticAccess, check_input},
        },
    },
    value::view::ValueView,
    value_check::scoped::CheckError,
};
use kagari_common::span::Span;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Shared completion contains no script value, body or cell edge. Failure merely
/// requests materialization; the provider continues scanning all physical fields.
#[derive(Debug)]
pub(crate) struct InitializationAttempt {
    completion: OnceLock<Result<(), StaticFailure>>,
    dirty: Arc<AtomicBool>,
    origin: Option<Span>,
}

impl InitializationAttempt {
    pub(in crate::module) fn new(dirty: Arc<AtomicBool>, origin: Option<Span>) -> Self {
        Self {
            completion: OnceLock::new(),
            dirty,
            origin,
        }
    }

    pub(in crate::module) fn failure(&self) -> Option<&StaticFailure> {
        self.completion
            .get()
            .and_then(|result| result.as_ref().err())
    }

    pub(in crate::module) fn fail(&self, cause: &str, origin: Option<Span>) {
        if self
            .completion
            .set(Err(StaticFailure::new(cause, origin.or(self.origin))))
            .is_ok()
        {
            self.dirty.store(true, Ordering::Release);
        }
    }
}

#[derive(Debug)]
pub(crate) struct StaticInitialization {
    access: StaticAccess,
    recipe: StaticRecipe,
    attempt: Arc<InitializationAttempt>,
    active: Arc<AtomicUsize>,
    started: bool,
}

/// A nonowning sealed commit witness, copied out of control before a source loan.
/// Its source prefix and callee result remain protected until the real frame exits.
#[derive(Debug, Clone)]
pub(crate) struct StaticCommit {
    access: StaticAccess,
    attempt: Arc<InitializationAttempt>,
}

impl StaticInitialization {
    pub(crate) fn recipe(&self) -> &StaticRecipe {
        &self.recipe
    }

    pub(crate) fn origin(&self) -> RevisionSource<'static> {
        RevisionSource::StaticRecipe {
            body: self.access.body,
            module: self.access.owner.slot(),
            slot: self.access.slot,
        }
    }

    pub(crate) fn commit_target(&self) -> StaticCommit {
        StaticCommit {
            access: self.access.clone(),
            attempt: self.attempt.clone(),
        }
    }

    pub(crate) fn begin(&mut self, runtime: &mut Runtime) -> Result<(), RuntimeError> {
        let count = self
            .active
            .load(Ordering::Relaxed)
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("static initialization depth"))?;
        if self.started {
            return Err(RuntimeError::module_validation(
                "static attempt already started",
            ));
        }
        self.access.with(runtime, |slots, cell| {
            StaticCellObject::begin(slots.scope(), cell, self.attempt.clone())?;
            Ok(())
        })?;
        self.active.store(count, Ordering::Relaxed);
        self.started = true;
        Ok(())
    }

    pub(crate) fn fail(&self, cause: &str) {
        if self.started {
            self.attempt.fail(cause, None);
        }
    }

    pub(crate) fn pending_failure(&self) -> Option<StaticCommit> {
        (self.started && self.attempt.failure().is_some()).then(|| self.commit_target())
    }
}

impl StaticCommit {
    pub(crate) fn commit(
        &self,
        runtime: &mut Runtime,
        value: &ValueView<'_>,
    ) -> Result<(), RuntimeError> {
        if let Some(failure) = self.attempt.failure() {
            return Err(RuntimeError::module_validation(format!(
                "static initialization failed: {failure}"
            )));
        }
        let Runtime {
            memory,
            resources,
            host,
            host_borrows,
            ..
        } = runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| {
                    let cell = self.access.resolve(slots)?;
                    let value = value.get(slots)?;
                    check_input(
                        slots.scope(),
                        value,
                        &self.access.owner,
                        self.access.slot,
                        host,
                        host_borrows,
                        resources,
                    )?;
                    resources.poll_closed()?;
                    StaticCellObject::complete(slots.scope(), cell, value, &self.attempt)?;
                    Ok::<_, CheckError>(())
                })())
            })
        });
        result
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| {
                runtime
                    .resources
                    .admit_execution_error(error.into_runtime(runtime))
            })?;
        // Runtime access is exclusive, completion has been checked and no callback
        // intervenes. A concurrent/duplicate completion is an internal invariant.
        self.attempt.completion.set(Ok(())).map_err(|_| {
            runtime
                .resources
                .quarantine(EngineFault::StaticReturnLostAttemptAuthority)
        })
    }

    pub(crate) fn fail(&self, cause: &str) {
        self.attempt.fail(cause, None);
    }
}

impl Drop for StaticInitialization {
    fn drop(&mut self) {
        if self.started {
            self.fail("static initializer execution abandoned");
            self.active.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

impl Runtime {
    pub(crate) fn static_initialization_active(&self) -> bool {
        self.resources
            .active_session()
            .is_some_and(|session| session.static_initializers.load(Ordering::Relaxed) != 0)
    }

    pub(crate) fn reject_static_initialization(&self, operation: &str) -> Result<(), RuntimeError> {
        if self.static_initialization_active() {
            return Err(RuntimeError::execution_phase_violation(format!(
                "{operation} during static initialization"
            )));
        }
        Ok(())
    }

    pub(crate) fn prepare_static_initialization(
        &mut self,
        access: StaticAccess,
    ) -> Result<Option<StaticInitialization>, RuntimeError> {
        self.validate_module_state_access(&access.owner)?;
        match self.observe_static(&access)? {
            Observation::Ready => Ok(None),
            Observation::Failed(failure) => Err(RuntimeError::module_validation(format!(
                "static initialization failed: {failure}"
            ))),
            Observation::Initializing(attempt) => {
                attempt.fail("recursive static initialization", None);
                self.flush_static_failures()?;
                Err(RuntimeError::module_validation(
                    "recursive static initialization",
                ))
            }
            Observation::Uninitialized(recipe) => {
                let active = self
                    .resources
                    .active_session()
                    .ok_or_else(|| {
                        RuntimeError::module_validation(
                            "static initialization requires an execution session",
                        )
                    })?
                    .static_initializers
                    .clone();
                let span = recipe.module.bytecode.functions[recipe.function.index()]
                    .metadata
                    .origins
                    .first()
                    .map(|origin| origin.span);
                let attempt = Arc::new(InitializationAttempt::new(
                    self.resources.static_failure_signal(),
                    span,
                ));
                Ok(Some(StaticInitialization {
                    access,
                    recipe,
                    attempt,
                    active,
                    started: false,
                }))
            }
        }
    }
}
