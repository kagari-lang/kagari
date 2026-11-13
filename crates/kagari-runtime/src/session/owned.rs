//! Owned roots retain session records while each driver activation borrows briefly.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, trap::TrapReason},
    execution::roots::SessionId,
    frame::{
        ExecutionStack, FrameStack, factory::QueuedFactory, types::arguments::TypeArgument,
        waiting::QueuedFuture,
    },
    module::ExecutableModule,
    native::future::claims::{ClaimedFuture, FutureContract},
    publication::ActivationMark,
    session::{
        ExecutionId, ExecutionOptions, ExecutionPhase, ExecutionSession, SessionScope, SessionState,
    },
    value::{carriers::RootedValue, properties::ValueProperties},
    value_check,
};

use kagari_bytecode::module::CallableTarget;
use kagari_common::cancellation::{CancellationSubscription, CancellationToken};
use kagari_contract::ids::FunctionRef;
use std::{
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Wake, Waker},
};

enum OwnedStart<'a> {
    Function(FunctionRef, &'a [RootedValue]),
    Future(QueuedFuture),
    Factory(QueuedFactory),
    Branch(QueuedFuture),
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

    pub(crate) fn cancellation_token(&self) -> CancellationToken {
        self.owner.cancellation.clone()
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.owner.cancellation.check().is_err()
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

/// Own admission until a parked handle is transferred to its caller. Declare this
/// before the activation so unwinding releases the active session before retirement.
struct OwnedInitialization<'runtime> {
    activation: ExecutionStack<'runtime>,
    execution: Option<OwnedExecution>,
}

/// Retain synchronous admission through descendant and provider cleanup without
/// holding a resource-field borrow over mutable Runtime operations.
struct ExecutionRetirement<'runtime> {
    runtime: &'runtime mut Runtime,
    activation: Option<ActivationMark>,
}

impl Drop for ExecutionRetirement<'_> {
    fn drop(&mut self) {
        if let Some(mark) = self.activation.take() {
            mark.retire(self.runtime.resources());
        }
    }
}

impl OwnedInitialization<'_> {
    fn retire(&mut self) -> Result<(), RuntimeError> {
        let Some(execution) = self.execution.take() else {
            return Ok(());
        };
        self.activation.retire_then(|runtime| {
            runtime.retirement_cleanup(|runtime| runtime.finish_owned_execution(&execution))
        })
    }
}

impl Drop for OwnedInitialization<'_> {
    fn drop(&mut self) {
        // Preserve the original unwind. Guarded retirement quarantines cleanup
        // faults without panicking again or requiring a subsequent driver turn.
        let _ = self.retire();
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
        &mut self,
        module: &ExecutableModule,
        entry: FunctionRef,
        args: &[RootedValue],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.require_idle_driver()?;
        self.drain_retired_executions()?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "owned candidate execution",
            ));
        }
        self.validate_executable_module(module)?;
        let route = module.bind_call_route(CallableTarget::Script(entry))?;
        let selected = self.select_call_route(&route)?;
        let CallableTarget::Script(entry) = selected.target() else {
            return Err(RuntimeError::module_validation(
                "owned script entry selected a native import",
            ));
        };
        let module = selected.owner();
        let function = module
            .bytecode
            .functions
            .get(entry.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid owned execution entry"))?;
        let parameters = (0..args.len())
            .map(|index| function.metadata.semantic.params.get(&index).cloned())
            .collect::<Option<Vec<_>>>()
            .filter(|_| args.len() == usize::from(function.parameter_count))
            .ok_or_else(|| RuntimeError::module_validation("invalid owned execution arguments"))?;
        let parameters = self.resolve_type_arguments(module, &parameters)?;
        for (value, expected) in args.iter().zip(&parameters) {
            let storable = self
                .memory
                .scope(&self.resources, |cx, _| {
                    let value = value.get(cx)?;
                    ValueProperties::of(cx, value).map(|facts| facts.storable)
                })
                .map_err(|error| self.managed_input_error(error))?;
            if !storable || !value_check::matches_rooted(self, module, expected, value)? {
                return Err(RuntimeError::module_validation(
                    "invalid owned execution arguments",
                ));
            }
        }
        self.create_owned_execution(module, options, OwnedStart::Function(entry, args))
    }

    /// Queue exactly one Future layer. Neither script code nor native submission
    /// runs until the host drives the returned owner.
    pub fn start_owned_future(
        &mut self,
        value: &RootedValue,
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.require_idle_driver()?;
        self.drain_retired_executions()?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "owned candidate Future",
            ));
        }
        let contract = self
            .memory
            .scope(&self.resources, |cx, _| {
                FutureContract::read(cx, value.get(cx)?)
            })
            .map_err(|error| self.managed_memory_error(error))?;
        self.validate_executable_module(&contract.owner)?;
        let queued = QueuedFuture::Unclaimed {
            output: contract.applied.parameter(self, &contract.owner, 0)?,
            value: value.clone(),
        };
        self.create_owned_execution(&contract.owner, options, OwnedStart::Future(queued))
    }

    /// Queue a zero-argument Future factory. Its ordinary body and exactly one
    /// returned Future layer run in this same execution on subsequent drives.
    pub fn start_owned_factory(
        &mut self,
        value: &RootedValue,
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

    pub(crate) fn start_queued_factory(
        &mut self,
        factory: QueuedFactory,
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.require_idle_driver()?;
        let owner = factory.owner.clone();
        self.create_owned_execution(&owner, options, OwnedStart::Factory(factory))
    }

    /// Prepare typed arguments inside the eventual invocation's session. The
    /// caller has entered using the retained callable's checked authority.
    pub(crate) fn initialize_owned_factory(
        session: ExecutionSession<'_>,
        external_cancellation: CancellationToken,
        prepare: impl FnOnce(&mut Runtime) -> Result<QueuedFactory, RuntimeError>,
    ) -> Result<OwnedExecution, RuntimeError> {
        let owner = session.root();
        Self::initialize_owned_execution(&owner, session, external_cancellation, |runtime| {
            prepare(runtime).map(OwnedStart::Factory)
        })
    }

    /// Reserve an inactive child without invoking observers, providers or script.
    pub(crate) fn prepare_owned_branch(
        &mut self,
        parent: SessionId,
        module: &ExecutableModule,
        output: TypeArgument,
    ) -> Result<OwnedExecution, RuntimeError> {
        self.require_idle_driver()?;
        let state = self
            .resources()
            .sessions
            .get(parent)
            .ok_or_else(|| RuntimeError::module_validation("group parent"))?;
        state.poll(self.resources())?;
        let options = state.options.clone();
        let nesting = state.group_depth.get() + 1;
        let logical_root = state.logical_root.get();
        drop(state);
        let queued = QueuedFuture::Reserved { output };
        let execution = self.create_owned_execution(module, options, OwnedStart::Branch(queued))?;
        let state = self
            .resources()
            .sessions
            .get(execution.id)
            .expect("reserved branch");
        state.group_depth.set(nesting);
        state.logical_root.set(logical_root);
        state.group_parent.set(Some(parent));
        Ok(execution)
    }

    pub(crate) fn install_branch_claim(&self, execution: &OwnedExecution, claim: ClaimedFuture) {
        let state = self
            .resources()
            .sessions
            .get(execution.id)
            .expect("reserved branch");
        let mut queued = state.queued_future.borrow_mut();
        let output = match queued.as_ref().expect("reserved branch start") {
            QueuedFuture::Reserved { output } => output.clone(),
            _ => unreachable!("branch claim installed once"),
        };
        *queued = Some(QueuedFuture::Claimed { claim, output });
    }

    fn create_owned_execution(
        &mut self,
        module: &ExecutableModule,
        options: ExecutionOptions,
        entry: OwnedStart<'_>,
    ) -> Result<OwnedExecution, RuntimeError> {
        let external_cancellation = options.cancellation;
        let options = ExecutionOptions {
            cancellation: CancellationToken::default(),
            ..options
        };
        let mut session = self.begin_execution(module, options)?;
        if !matches!(&entry, OwnedStart::Branch(_)) {
            session.runtime_mut().attach_execution_observer()?;
        }
        Self::initialize_owned_execution(module, session, external_cancellation, |_| Ok(entry))
    }

    fn initialize_owned_execution<'args>(
        module: &ExecutableModule,
        session: ExecutionSession<'_>,
        external_cancellation: CancellationToken,
        prepare: impl FnOnce(&mut Runtime) -> Result<OwnedStart<'args>, RuntimeError>,
    ) -> Result<OwnedExecution, RuntimeError> {
        let id = session.scope.id;
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
        let mut initialization = OwnedInitialization {
            activation: ExecutionStack::new(session)?,
            execution: Some(OwnedExecution {
                id,
                owner: owner.clone(),
            }),
        };
        let initialized = initialization.activation.with(|runtime, stack| {
            *runtime
                .resources()
                .sessions
                .get(id)
                .expect("new session")
                .owner
                .borrow_mut() = Some(owner);
            match prepare(runtime)? {
                OwnedStart::Function(function, args) => stack.push_rooted_callable(
                    runtime,
                    module.clone(),
                    CallableTarget::Script(function),
                    args,
                ),
                OwnedStart::Future(queued) | OwnedStart::Branch(queued) => {
                    *runtime
                        .resources()
                        .sessions
                        .get(id)
                        .expect("new session")
                        .queued_future
                        .borrow_mut() = Some(queued);
                    Ok(())
                }
                OwnedStart::Factory(queued) => {
                    *runtime
                        .resources()
                        .sessions
                        .get(id)
                        .expect("new session")
                        .queued_factory
                        .borrow_mut() = Some(queued);
                    Ok(())
                }
            }
        });
        if let Err(error) = initialized {
            initialization.retire()?;
            return Err(error);
        }
        let parked = initialization.activation.drive(|runtime, frames| {
            let result = FrameStack::try_park_ready(frames, runtime).and_then(|parked| {
                if parked {
                    Ok(())
                } else {
                    Err(RuntimeError::module_validation(
                        "execution has non-suspendable resources",
                    ))
                }
            });
            result.map_err(|error| runtime.resources().admit_execution_error(error))
        });
        if let Err(error) = parked {
            initialization.retire()?;
            return Err(error);
        }
        Ok(initialization.execution.take().expect("initialized owner"))
    }

    /// Activate an existing stack. Independent and recursive roots cannot overlap.
    pub fn resume_owned_execution(
        &mut self,
        owner: &OwnedExecution,
    ) -> Result<ExecutionStack<'_>, RuntimeError> {
        self.require_idle_driver()?;
        self.ensure_scheduler_open()?;
        self.validate_owned_execution(owner)?;
        let activation = self.resources().begin_synchronous_activation()?;
        let state = self
            .resources()
            .sessions
            .get(owner.id)
            .expect("validated owner");
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
        let scope = SessionScope::new(owner.id, activation.detach());
        let mut session = ExecutionSession {
            runtime: self,
            scope,
        };
        session.runtime_mut().resume_execution_observer()?;
        ExecutionStack::resume(session)
    }

    pub(crate) fn validate_owned_execution(
        &self,
        owner: &OwnedExecution,
    ) -> Result<(), RuntimeError> {
        let state = self
            .resources()
            .sessions
            .get(owner.id)
            .ok_or_else(|| RuntimeError::module_validation("foreign or retired execution"))?;
        if state.scopes.get() != 0
            || state
                .owner
                .borrow()
                .as_ref()
                .is_none_or(|held| !Arc::ptr_eq(held, &owner.owner))
        {
            return Err(RuntimeError::module_validation(
                "foreign or active execution owner",
            ));
        }
        Ok(())
    }

    /// Release a terminal execution after its activation and output rooting end.
    pub fn finish_owned_execution(&mut self, owner: &OwnedExecution) -> Result<(), RuntimeError> {
        self.require_idle_driver()?;
        self.validate_owned_execution(owner)?;
        self.retire_execution(owner.id)
    }

    /// Complete owner-drop cleanup even if a provider or dispatcher never replies.
    /// A cleanup fault quarantines execution but does not stop the drain.
    pub fn drain_retired_executions(&mut self) -> Result<usize, RuntimeError> {
        self.drain_owned_executions(self.resources().is_quarantined())
    }

    pub(crate) fn drain_owned_executions(&mut self, all: bool) -> Result<usize, RuntimeError> {
        self.require_idle_driver()?;
        let ids = self.resources().sessions.retired_owners(all);
        let mut count = ids.len();
        let mut failure = None;
        for id in ids {
            // Parent teardown may already have retired this descendant.
            if self.resources().sessions.get(id).is_none() {
                continue;
            }
            if let Err(error) = self.retire_execution(id) {
                failure.get_or_insert(error);
            }
        }
        // A fault discovered in this batch invalidates even retained owners.
        // Drain those newly ineligible sessions once, without running their code.
        if !all && self.resources().is_quarantined() {
            match self.drain_owned_executions(true) {
                Ok(retired) => count += retired,
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(count), Err)
    }

    pub(crate) fn require_idle_driver(&self) -> Result<(), RuntimeError> {
        self.resources().ensure_synchronous_thread()?;
        self.resources().ensure_not_publishing()?;
        if self.resources().active_session().is_some() {
            return Err(RuntimeError::from(TrapReason::RuntimeDriverAlreadyActive));
        }
        Ok(())
    }

    fn retire_execution(&mut self, id: SessionId) -> Result<(), RuntimeError> {
        let activation = self.resources().begin_synchronous_activation()?.detach();
        let retirement = ExecutionRetirement {
            runtime: self,
            activation: Some(activation),
        };
        let runtime = &mut *retirement.runtime;
        let group_failure = runtime.close_owned_groups(id).err();
        let (mut state, mut frames) = runtime.resources().sessions.remove(id).ok_or_else(|| {
            RuntimeError::module_validation("foreign, retired or borrowed execution")
        })?;
        let mut failed = group_failure.is_some();
        if let Some(mut wait) = state.pending.get_mut().take() {
            failed |= runtime.retirement_cleanup(|_| wait.cancel()).is_err();
            failed |= runtime
                .retirement_cleanup(|_| {
                    drop(wait);
                    Ok(())
                })
                .is_err();
        }
        // Release every window before dropping any frame's retained host owners.
        for frame in frames.iter_mut().rev() {
            frame.release_values(&mut runtime.memory, &runtime.resources);
        }
        if let Err(error) = runtime.memory.retire_execution(&runtime.resources, id) {
            // A faulted heap keeps inert source carriers until shutdown. Control,
            // host loans and wakers still retire below without reopening access.
            if !runtime.resources().is_quarantined() {
                runtime
                    .resources()
                    .quarantine(EngineFault::ExecutionStorageRetirementFailed(error));
            }
            failed = true;
        }
        for mut frame in frames {
            failed |= runtime
                .retirement_cleanup(|runtime| {
                    frame.release_control(&runtime.resources);
                    drop(frame);
                    Ok(())
                })
                .is_err();
        }
        // A host control waker can have a panicking destructor too. Detach it
        // before SessionState's Drop so a failed finish is never retried there.
        if let Some(owner) = state.owner.get_mut().take() {
            failed |= runtime
                .retirement_cleanup(|_| {
                    owner.finish();
                    Ok(())
                })
                .is_err();
            failed |= runtime
                .retirement_cleanup(|_| {
                    drop(owner);
                    Ok(())
                })
                .is_err();
        }
        failed |= runtime
            .retirement_cleanup(|_| {
                drop(state);
                Ok(())
            })
            .is_err();
        if failed {
            Err(runtime.quarantine_execution_invariant(EngineFault::OwnedExecutionCleanupFailed))
        } else {
            Ok(())
        }
    }

    pub(crate) fn retirement_cleanup(
        &mut self,
        cleanup: impl FnOnce(&mut Runtime) -> Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        match catch_unwind(AssertUnwindSafe(|| cleanup(self))) {
            Ok(Ok(())) => Ok(()),
            result => {
                let error =
                    self.quarantine_execution_invariant(EngineFault::OwnedExecutionCleanupFailed);
                if let Err(payload) = result
                    && let Err(nested) = catch_unwind(AssertUnwindSafe(|| drop(payload)))
                {
                    // A second hostile panic payload cannot be safely destroyed.
                    mem::forget(nested);
                }
                Err(error)
            }
        }
    }
}
