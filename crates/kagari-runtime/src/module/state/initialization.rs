//! Frame-owned initialization attempts. Completion is detached from storage borrows
//! so a cycle or dropped/terminated frame can poison immediately, including reentry.
use crate::{
    Runtime,
    error::RuntimeError,
    module::{
        LoadedModule,
        state::{StaticFailure, StaticRecipe, StaticState},
    },
    value::Value,
};
use kagari_bytecode::instruction::ModuleSlot;
use kagari_common::span::Span;
use std::{
    slice,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug, Default)]
pub(crate) struct InitializationAttempt {
    completion: OnceLock<Result<(), StaticFailure>>,
}

impl InitializationAttempt {
    pub(in crate::module) fn failure(&self) -> Option<&StaticFailure> {
        self.completion
            .get()
            .and_then(|result| result.as_ref().err())
    }

    fn fail(&self, cause: &str, origin: Option<Span>) {
        let _ = self.completion.set(Err(StaticFailure::new(cause, origin)));
    }
}

/// Private authority attached only to an admitted initializer frame. The cell and
/// frame share completion, not mutable values or a lease on every historical program.
#[derive(Debug)]
pub(crate) struct StaticInitialization {
    owner: LoadedModule,
    slot: ModuleSlot,
    recipe: StaticRecipe,
    attempt: Arc<InitializationAttempt>,
    active: Arc<AtomicUsize>,
    started: bool,
    origin: Option<Span>,
}

impl StaticInitialization {
    pub(crate) fn recipe(&self) -> &StaticRecipe {
        &self.recipe
    }

    pub(crate) fn begin(&mut self, runtime: &Runtime) -> Result<(), RuntimeError> {
        let mut store = runtime.modules.inner.try_borrow_mut().map_err(|_| {
            runtime
                .resources()
                .quarantine("static store borrowed during initialization")
        })?;
        let record = store
            .resolve(&self.owner)
            .ok_or_else(|| RuntimeError::module_validation("static initializer owner retired"))?;
        store.bound_cell(record, self.slot.index()).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid static initializer binding")
        })?;
        let id = record.bindings[self.slot.index()];
        let cell = store.cells.get_mut(&id).expect("checked cell");
        if !matches!(cell.state, StaticState::Uninitialized(_)) || cell.attempt.is_some() {
            return Err(RuntimeError::module_validation(
                "static attempt already started",
            ));
        }
        let count = self
            .active
            .load(Ordering::Relaxed)
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("static initialization depth"))?;
        cell.state = StaticState::Initializing(self.recipe.clone());
        cell.attempt = Some(self.attempt.clone());
        self.active.store(count, Ordering::Relaxed);
        self.started = true;
        Ok(())
    }

    /// Validation cannot execute callbacks or collect. The return transition retains
    /// an explicit result root across retirement, admission and this closed commit.
    pub(crate) fn commit(&self, runtime: &Runtime, value: Value) -> Result<(), RuntimeError> {
        if let Some(failure) = self.attempt.failure() {
            return Err(RuntimeError::module_validation(format!(
                "static initialization failed: {failure}"
            )));
        }
        let contract = &self.owner.bytecode.module_slots[self.slot.index()].contract;
        runtime.validate_heap_payloads(slice::from_ref(&value))?;
        if !runtime.matches_type_in(&value, &contract.ty, &self.owner, None) {
            return Err(RuntimeError::module_validation(
                "static initializer semantic type mismatch",
            ));
        }
        let mut store = runtime.modules.inner.try_borrow_mut().map_err(|_| {
            runtime
                .resources()
                .quarantine("static store borrowed during initializer return")
        })?;
        let record = store
            .resolve(&self.owner)
            .ok_or_else(|| RuntimeError::module_validation("static owner retired"))?;
        store.bound_cell(record, self.slot.index()).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid static return binding")
        })?;
        let id = record.bindings[self.slot.index()];
        let cell = store.cells.get_mut(&id).expect("checked cell");
        if !matches!(cell.state, StaticState::Initializing(_))
            || !cell
                .attempt
                .as_ref()
                .is_some_and(|attempt| Arc::ptr_eq(attempt, &self.attempt))
        {
            return Err(runtime
                .resources()
                .quarantine("static return lost attempt authority"));
        }
        self.attempt
            .completion
            .set(Ok(()))
            .map_err(|_| RuntimeError::module_validation("static initialization already failed"))?;
        cell.state = StaticState::Ready(value);
        cell.attempt = None;
        Ok(())
    }

    pub(crate) fn fail(&self, cause: &str) {
        if self.started {
            self.attempt.fail(cause, self.origin);
        }
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
        self.resources()
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
        &self,
        owner: &LoadedModule,
        slot: ModuleSlot,
    ) -> Result<Option<StaticInitialization>, RuntimeError> {
        self.validate_module_state_access(owner)?;
        let state = self
            .modules
            .static_state(owner, slot)
            .ok_or_else(|| RuntimeError::module_validation("invalid static binding"))?;
        match state {
            StaticState::Ready(_) => Ok(None),
            StaticState::Failed(failure) => Err(RuntimeError::module_validation(format!(
                "static initialization failed: {failure}"
            ))),
            StaticState::Initializing(_) => {
                self.poison_static_cycle(owner, slot)?;
                Err(RuntimeError::module_validation(
                    "recursive static initialization",
                ))
            }
            StaticState::Uninitialized(recipe) => {
                let active = self
                    .resources()
                    .active_session()
                    .ok_or_else(|| {
                        RuntimeError::module_validation(
                            "static initialization requires an execution session",
                        )
                    })?
                    .static_initializers
                    .clone();
                let origin = recipe.module.bytecode.functions[recipe.function.index()]
                    .metadata
                    .origins
                    .first()
                    .map(|origin| origin.span);
                Ok(Some(StaticInitialization {
                    owner: owner.clone(),
                    slot,
                    recipe,
                    active,
                    origin,
                    attempt: Arc::new(InitializationAttempt::default()),
                    started: false,
                }))
            }
        }
    }

    pub(in crate::module) fn poison_static_cycle(
        &self,
        owner: &LoadedModule,
        slot: ModuleSlot,
    ) -> Result<(), RuntimeError> {
        let store = self.modules.inner.try_borrow().map_err(|_| {
            self.resources()
                .quarantine("static store borrowed during cycle detection")
        })?;
        let record = store
            .resolve(owner)
            .ok_or_else(|| RuntimeError::module_validation("static owner retired"))?;
        let cell = store
            .bound_cell(record, slot.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid static binding"))?;
        if let Some(attempt) = &cell.attempt {
            let origin = match &cell.state {
                StaticState::Initializing(recipe) => recipe.module.bytecode.functions
                    [recipe.function.index()]
                .metadata
                .origins
                .first()
                .map(|origin| origin.span),
                _ => None,
            };
            attempt.fail("recursive static initialization", origin);
        }
        Ok(())
    }
}
