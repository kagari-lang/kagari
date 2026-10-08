//! Owned roots retain session records while each driver activation borrows briefly.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionStack,
    module::LoadedModule,
    session::{ExecutionOptions, ExecutionPhase, ExecutionSession, store::SessionId},
    value::Value,
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::ids::FunctionRef;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::Waker,
};

#[derive(Debug)]
pub(crate) struct ExecutionOwner {
    abandoned: AtomicBool,
    cancellation: CancellationToken,
    wake: Mutex<Option<Waker>>,
}

impl ExecutionOwner {
    pub(crate) fn abandoned(&self) -> bool {
        self.abandoned.load(Ordering::Acquire)
    }

    fn notify(&self) {
        let wake = self.wake.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(wake) = wake {
            wake.wake();
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
    pub fn cancel(&self) {
        self.owner.cancellation.cancel();
        self.owner.notify();
    }

    /// Register a host control wakeup without allowing it to enter the runtime.
    pub fn set_waker(&self, wake: &Waker) {
        *self.owner.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake.clone());
        if self.owner.cancellation.check().is_err() {
            self.owner.notify();
        }
    }
}

impl Drop for OwnedExecution {
    fn drop(&mut self) {
        self.owner.abandoned.store(true, Ordering::Release);
        self.owner.notify();
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
        let session = self.begin_execution(module, options)?;
        self.attach_execution_observer()?;
        let id = session.id;
        let owner = Arc::new(ExecutionOwner {
            abandoned: AtomicBool::new(false),
            cancellation: session.state().options.cancellation.clone(),
            wake: Mutex::new(None),
        });
        let stack = ExecutionStack::new(session)?;
        stack.push(self, module.slot(), entry, args, None)?;
        *self
            .resources()
            .sessions
            .get(id)
            .expect("new session")
            .owner
            .borrow_mut() = Some(owner.clone());
        let handle = OwnedExecution { id, owner };
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
        self.resources().start_execution(owner.id);
        self.resources()
            .restore_call_depth(state.parked_depth.replace(0));
        drop(state);
        let session = ExecutionSession {
            id: owner.id,
            resources: self.resources(),
        };
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

    fn require_idle_driver(&self) -> Result<(), RuntimeError> {
        if self.resources().active_session().is_some() {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "runtime driver already active",
            ));
        }
        Ok(())
    }

    fn retire_execution(&self, id: SessionId) -> Result<(), RuntimeError> {
        let (state, frames) = self.resources().sessions.remove(id).ok_or_else(|| {
            RuntimeError::module_validation("foreign, retired or borrowed execution")
        })?;
        for frame in frames {
            frame.release_values(self.resources());
        }
        drop(state);
        Ok(())
    }
}
