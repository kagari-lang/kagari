//! Owned roots retain session records while each driver activation borrows briefly.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::{ExecutionStack, factory::QueuedFactory, waiting::QueuedFuture},
    module::LoadedModule,
    session::{
        ExecutionId, ExecutionOptions, ExecutionPhase, ExecutionSession, SessionState,
        store::SessionId,
    },
    value::Value,
};
use kagari_common::cancellation::{CancellationSubscription, CancellationToken};
use kagari_contract::ids::FunctionRef;
use std::{
    slice,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Wake, Waker},
};

enum OwnedStart<'a> {
    Function(FunctionRef, &'a [Value]),
    Future(QueuedFuture),
    Factory(QueuedFactory),
}

#[derive(Debug)]
pub(crate) struct ExecutionOwner {
    abandoned: AtomicBool,
    ready: AtomicBool,
    finished: AtomicBool,
    cancellation: CancellationToken,
    cancellation_wake: OnceLock<CancellationSubscription>,
    external_cancellation: OnceLock<CancellationSubscription>,
    wake: Mutex<Option<Arc<Waker>>>,
}

impl ExecutionOwner {
    pub(crate) fn abandoned(&self) -> bool {
        self.abandoned.load(Ordering::Acquire)
    }

    fn notify(&self) {
        let wake = self.wake.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(wake) = wake {
            wake.wake_by_ref();
        }
    }

    pub(crate) fn mark_ready(&self) {
        if !self.finished.load(Ordering::Acquire) && !self.ready.swap(true, Ordering::AcqRel) {
            self.notify();
        }
    }

    pub(crate) fn finish(&self) {
        self.finished.store(true, Ordering::Release);
        self.ready.store(false, Ordering::Release);
        let wake = self.wake.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(wake);
    }

    pub(crate) fn waker(self: &Arc<Self>) -> Waker {
        Waker::from(Arc::new(ExecutionWake(Arc::downgrade(self))))
    }
}

struct ExecutionWake(Weak<ExecutionOwner>);

impl Wake for ExecutionWake {
    fn wake(self: Arc<Self>) {
        if let Some(owner) = self.0.upgrade() {
            owner.mark_ready();
        }
    }
}

struct ExternalCancellation(Weak<ExecutionOwner>);

impl Wake for ExternalCancellation {
    fn wake(self: Arc<Self>) {
        if let Some(owner) = self.0.upgrade() {
            owner.cancellation.cancel();
        }
    }
}

/// An exclusive execution owner, independent of the runtime's Rust address.
/// Drop requests retirement; the owner thread must drain or drive the runtime.
#[must_use = "drive this execution or drop it and drain retired executions"]
#[derive(Debug)]
pub struct OwnedExecution {
    pub(crate) id: SessionId,
    owner: Arc<ExecutionOwner>,
}

impl OwnedExecution {
    /// Detached identity for correlation across drive calls; grants no entry authority.
    pub fn execution_id(&self) -> ExecutionId {
        ExecutionId(self.id)
    }

    /// Request termination of this execution only. The host-supplied token may
    /// cancel several executions, but local cancellation never propagates to it.
    pub fn cancel(&self) {
        self.owner.cancellation.cancel();
    }

    /// Readiness is durable and coalesced. A wake is only a scheduling hint.
    pub fn is_ready(&self) -> bool {
        !self.owner.finished.load(Ordering::Acquire)
            && (self.owner.ready.load(Ordering::Acquire)
                || self.owner.cancellation.check().is_err())
    }

    /// Register a host control wakeup without allowing it to enter the runtime.
    pub fn set_waker(&self, wake: &Waker) {
        let wake = Arc::new(wake.clone());
        let previous = {
            let mut target = self.owner.wake.lock().unwrap_or_else(|e| e.into_inner());
            if self.owner.finished.load(Ordering::Acquire) {
                return;
            }
            target.replace(wake)
        };
        drop(previous);
        if self.is_ready() {
            self.owner.notify();
        }
    }
}

impl Drop for OwnedExecution {
    fn drop(&mut self) {
        self.owner.abandoned.store(true, Ordering::Release);
        self.owner.mark_ready();
    }
}

impl Drop for SessionState {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.get_mut().as_ref() {
            owner.finish();
        }
    }
}

impl Runtime {
    /// Reserve a checked entry and its rooted arguments without running script.
    pub fn start_owned_execution(
        &self,
        module: &LoadedModule,
        entry: FunctionRef,
        args: &[Value],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.require_idle_driver()?;
        self.drain_retired_executions()?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "owned candidate execution",
            ));
        }
        let function = module
            .bytecode
            .functions
            .get(entry.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid owned execution entry"))?;
        if args.len() != usize::from(function.parameter_count)
            || args.iter().enumerate().any(|(index, value)| {
                !value.is_storable()
                    || function
                        .metadata
                        .semantic
                        .locals
                        .get(&index)
                        .is_none_or(|ty| !self.matches_type_in(value, ty, module, None))
            })
        {
            return Err(RuntimeError::module_validation(
                "invalid owned execution arguments",
            ));
        }
        self.create_owned_execution(module, options, OwnedStart::Function(entry, args))
    }

    /// Queue exactly one Future layer. Neither script code nor native submission
    /// runs until the host drives the returned owner.
    pub fn start_owned_future(
        &self,
        value: &Value,
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.require_idle_driver()?;
        self.drain_retired_executions()?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "owned candidate Future",
            ));
        }
        let Value::GcHandle(id) = value else {
            return Err(RuntimeError::module_validation(
                "owned entry requires a Future",
            ));
        };
        let (owner, ty, scope) = self.gc().future_contract(*id)?;
        self.validate_loaded_module(&owner)?;
        let future = match scope {
            Some(scope) => scope,
            None => self
                .resolve_type_arguments(&owner, slice::from_ref(&ty))?
                .pop()
                .ok_or_else(|| RuntimeError::module_validation("Future entry type"))?,
        };
        let queued = QueuedFuture {
            output: future.parameter(self, &owner, 0)?,
            value: self
                .root_value(value.clone())
                .ok_or_else(|| RuntimeError::module_validation("Future entry root"))?,
        };
        self.create_owned_execution(&owner, options, OwnedStart::Future(queued))
    }

    /// Queue a zero-argument Future factory. Its ordinary body and exactly one
    /// returned Future layer run in this same execution on subsequent drives.
    pub fn start_owned_factory(
        &self,
        value: &Value,
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.require_idle_driver()?;
        self.drain_retired_executions()?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "owned candidate factory",
            ));
        }
        let queued = self.prepare_future_factory(value)?;
        let owner = queued.owner.clone();
        self.create_owned_execution(&owner, options, OwnedStart::Factory(queued))
    }

    fn create_owned_execution(
        &self,
        module: &LoadedModule,
        options: ExecutionOptions,
        entry: OwnedStart<'_>,
    ) -> Result<OwnedExecution, RuntimeError> {
        let external_cancellation = options.cancellation;
        let options = ExecutionOptions {
            cancellation: CancellationToken::default(),
            ..options
        };
        let session = self.begin_execution(module, options)?;
        self.attach_execution_observer()?;
        let id = session.id;
        let owner = Arc::new(ExecutionOwner {
            abandoned: AtomicBool::new(false),
            ready: AtomicBool::new(true),
            finished: AtomicBool::new(false),
            cancellation: session.state().options.cancellation.clone(),
            cancellation_wake: OnceLock::new(),
            external_cancellation: OnceLock::new(),
            wake: Mutex::new(None),
        });
        owner
            .cancellation_wake
            .set(owner.cancellation.subscribe(owner.waker()))
            .expect("new cancellation wake registration");
        owner
            .external_cancellation
            .set(
                external_cancellation.subscribe(Waker::from(Arc::new(ExternalCancellation(
                    Arc::downgrade(&owner),
                )))),
            )
            .expect("new external cancellation registration");
        let stack = ExecutionStack::new(session)?;
        *self
            .resources()
            .sessions
            .get(id)
            .expect("new session")
            .owner
            .borrow_mut() = Some(owner.clone());
        let handle = OwnedExecution { id, owner };
        let initialized = match entry {
            OwnedStart::Function(function, args) => {
                stack.push(self, module.slot(), function, args, None)
            }
            OwnedStart::Future(queued) => {
                *self
                    .resources()
                    .sessions
                    .get(id)
                    .expect("new session")
                    .queued_future
                    .borrow_mut() = Some(queued);
                Ok(())
            }
            OwnedStart::Factory(queued) => {
                *self
                    .resources()
                    .sessions
                    .get(id)
                    .expect("new session")
                    .queued_factory
                    .borrow_mut() = Some(queued);
                Ok(())
            }
        };
        if let Err(error) = initialized {
            drop(stack);
            self.finish_owned_execution(&handle)?;
            return Err(error);
        }
        if let Err(error) = stack.park(self) {
            self.finish_owned_execution(&handle)?;
            return Err(error);
        }
        Ok(handle)
    }

    /// Activate an existing stack. Independent and recursive roots cannot overlap.
    pub fn resume_owned_execution(
        &self,
        owner: &OwnedExecution,
    ) -> Result<ExecutionStack<'_>, RuntimeError> {
        self.require_idle_driver()?;
        self.gc().ensure_no_native_borrow()?;
        let state = self
            .resources()
            .sessions
            .get(owner.id)
            .ok_or_else(|| RuntimeError::module_validation("foreign or retired execution"))?;
        if state
            .owner
            .borrow()
            .as_ref()
            .is_none_or(|held| !Arc::ptr_eq(held, &owner.owner))
            || state.scopes.get() != 0
        {
            return Err(RuntimeError::module_validation(
                "execution is already active",
            ));
        }
        state.scopes.set(1);
        owner.owner.ready.store(false, Ordering::Release);
        self.resources().start_execution(owner.id);
        self.resources()
            .restore_call_depth(state.parked_depth.replace(0));
        drop(state);
        let session = ExecutionSession {
            id: owner.id,
            resources: self.resources(),
        };
        self.resume_execution_observer()?;
        ExecutionStack::resume(session)
    }

    /// Release a terminal execution after its activation and output rooting end.
    pub fn finish_owned_execution(&self, owner: &OwnedExecution) -> Result<(), RuntimeError> {
        self.require_idle_driver()?;
        self.retire_execution(owner.id)
    }

    /// Complete owner-drop cleanup even if a provider or dispatcher never replies.
    pub fn drain_retired_executions(&self) -> Result<usize, RuntimeError> {
        self.require_idle_driver()?;
        let ids = self
            .resources()
            .sessions
            .retired_owners(self.resources().is_quarantined());
        let count = ids.len();
        for id in ids {
            self.retire_execution(id)?;
        }
        Ok(count)
    }

    pub(crate) fn require_idle_driver(&self) -> Result<(), RuntimeError> {
        if self.resources().active_session().is_some() {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "runtime driver already active",
            ));
        }
        Ok(())
    }

    fn retire_execution(&self, id: SessionId) -> Result<(), RuntimeError> {
        let (mut state, frames) = self.resources().sessions.remove(id).ok_or_else(|| {
            RuntimeError::module_validation("foreign, retired or borrowed execution")
        })?;
        let cleanup = state
            .pending
            .get_mut()
            .take()
            .map(|mut wait| wait.cancel())
            .transpose();
        for frame in frames {
            frame.release_values(self.resources());
        }
        drop(state);
        cleanup
            .map(|_| ())
            .map_err(|_| self.quarantine_execution_invariant("native operation cleanup failed"))
    }
}
