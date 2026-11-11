//! Checked stack control contains identities and marks, never borrowed storage.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::{ExecutionFrame, FrameDispatch, FrameEntry, FrameStack, arguments::FrameArguments},
    module::ExecutableModule,
    session::{SessionState, store::SessionId},
    value::Value,
};
use kagari_bytecode::{instruction::Slot, module::CallableTarget, program::ModuleRef};
use kagari_contract::ids::FunctionRef;
use std::cell::{Ref, RefMut};

impl FrameStack {
    pub(crate) fn new(runtime: &Runtime, session: SessionId) -> Result<Self, RuntimeError> {
        let base = runtime
            .resources()
            .sessions
            .frames(session)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::FrameStackIsBorrowedAcrossExecution)
            })?
            .len();
        let state = runtime
            .resources()
            .sessions
            .get(session)
            .expect("live execution session");
        let id = state.next_frame_scope.get();
        state.next_frame_scope.set(id.checked_add(1).ok_or_else(|| {
            runtime
                .resources()
                .quarantine(EngineFault::FrameScopeIdentityExhausted)
        })?);
        state.frame_scopes.borrow_mut().push(id);
        drop(state);
        Ok(Self {
            _thread: Default::default(),
            session,
            base,
            id,
            parked: false,
        })
    }

    fn validate_owner(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        if runtime.resources().sessions.get(self.session).is_none() {
            return Err(RuntimeError::module_validation(
                "execution stack belongs to another runtime",
            ));
        }
        Ok(())
    }

    pub(super) fn session_state<'r>(&self, runtime: &'r Runtime) -> Ref<'r, SessionState> {
        runtime
            .resources()
            .sessions
            .get(self.session)
            .expect("live execution scope")
    }

    pub(super) fn validate_top(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.validate_owner(runtime)?;
        runtime.resources().ensure_execution_allowed()?;
        if !runtime
            .resources()
            .active_session()
            .is_some_and(|active| active.id == self.session)
        {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::ExecutionUsedASuspendedSession));
        }
        if self.session_state(runtime).frame_scopes.borrow().last() != Some(&self.id) {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::ExecutionUsedASuspendedFrameScope));
        }
        Ok(())
    }

    pub fn frames<'r>(
        &self,
        runtime: &'r Runtime,
    ) -> Result<Ref<'r, Vec<ExecutionFrame>>, RuntimeError> {
        self.validate_owner(runtime)?;
        runtime
            .resources()
            .sessions
            .frames(self.session)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::FrameStackIsMutablyBorrowed)
            })
    }

    pub fn current<'r>(
        &self,
        runtime: &'r Runtime,
    ) -> Result<Ref<'r, ExecutionFrame>, RuntimeError> {
        self.validate_top(runtime)?;
        let frames = self.frames(runtime)?;
        if frames.len() <= self.base {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::MissingExecutionFrame));
        }
        Ok(Ref::map(frames, |frames| {
            frames.last().expect("checked frame")
        }))
    }

    pub fn current_mut<'r>(
        &self,
        runtime: &'r Runtime,
    ) -> Result<RefMut<'r, ExecutionFrame>, RuntimeError> {
        self.validate_top(runtime)?;
        let frames = runtime
            .resources()
            .sessions
            .frames_mut(self.session)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::FrameStackIsBorrowedAcrossExecution)
            })?;
        if frames.len() <= self.base {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::MissingExecutionFrame));
        }
        Ok(RefMut::map(frames, |frames| {
            frames.last_mut().expect("checked frame")
        }))
    }

    pub fn push(
        &self,
        runtime: &Runtime,
        module: ModuleRef,
        function: FunctionRef,
        args: &[Value],
        return_dst: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let loaded = {
            let frames = self.frames(runtime)?;
            let caller = (frames.len() > self.base)
                .then(|| frames.last().map(ExecutionFrame::loaded))
                .flatten();
            match caller {
                Some(caller) => caller.member(module),
                None => self.session_state(runtime).root.clone().member(module),
            }
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::InvalidFrameModule)
            })?
        };
        let route = loaded.bind_call_route(CallableTarget::Script(function))?;
        let selected = runtime.select_call_route(&route)?;
        self.push_callable(
            runtime,
            selected.owner().clone(),
            selected.target(),
            args,
            return_dst,
        )
    }

    /// Enters the current method selected by the rooted interface's checked evidence.
    pub fn push_interface_method(
        &self,
        runtime: &Runtime,
        method: RootedInterfaceMethod,
        args: &[Value],
        return_dst: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let method = method.admit(runtime)?;
        let view = method.view(runtime)?;
        runtime.validate_executable_module(view.implementation())?;
        let arguments = FrameArguments::plain(args);
        view.validate_arguments(runtime, arguments)?;
        let loaded = view.implementation().clone();
        let target = view.target();
        let environment = view.environment().cloned();
        drop(view);
        // The host lease remains alive until the window has published all edges.
        self.push_admitted_arguments(
            runtime,
            loaded,
            target,
            arguments,
            return_dst,
            FrameDispatch {
                prepared: None,
                entry: FrameEntry::Call,
                invocation: Some(method.invocation),
                environment,
            },
        )
        .map(|_| ())
    }

    /// Enter an already selected exact callable. Retained computations and prepared
    /// host entries must not resolve current code again at this admission boundary.
    pub fn push_callable(
        &self,
        runtime: &Runtime,
        loaded: ExecutableModule,
        target: CallableTarget,
        args: &[Value],
        return_dst: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        self.push_arguments(
            runtime,
            loaded,
            target,
            FrameArguments::plain(args),
            return_dst,
            FrameDispatch {
                prepared: None,
                entry: FrameEntry::Call,
                invocation: None,
                environment: None,
            },
        )
    }

    pub(super) fn push_arguments(
        &self,
        runtime: &Runtime,
        loaded: ExecutableModule,
        target: CallableTarget,
        args: FrameArguments<'_>,
        return_dst: Option<Slot>,
        dispatch: FrameDispatch<'_>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        self.push_admitted_arguments(runtime, loaded, target, args, return_dst, dispatch)
            .map(|_| ())
    }

    pub fn pop(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let mut frames = runtime
            .resources()
            .sessions
            .frames_mut(self.session)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::FrameStackIsBorrowedAcrossExecution)
            })?;
        if frames.len() <= self.base {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::FrameScopeAttemptedToPopItsCaller));
        }
        if let Some(frame) = frames.pop() {
            frame.release_values(runtime.resources());
        }
        runtime.resources().leave_call();
        Ok(())
    }

    pub fn is_empty(&self, runtime: &Runtime) -> Result<bool, RuntimeError> {
        Ok(self.frames(runtime)?.len() == self.base)
    }
}

impl FrameStack {
    pub(crate) fn retire(&mut self, runtime: &Runtime) {
        let state = self.session_state(runtime);
        let mut scopes = state.frame_scopes.borrow_mut();
        let Some(position) = scopes.iter().position(|id| *id == self.id) else {
            return;
        };
        if position + 1 != scopes.len() {
            runtime
                .resources()
                .quarantine(EngineFault::FrameScopesDroppedOutOfOrder);
        }
        scopes.truncate(position);
        if self.parked {
            return;
        }
        let Some(mut frames) = runtime.resources().sessions.frames_mut(self.session) else {
            runtime
                .resources()
                .quarantine(EngineFault::FrameStackRemainedBorrowedDuringCleanup);
            return;
        };
        while frames.len() > self.base {
            if let Some(frame) = frames.pop() {
                frame.release_values(runtime.resources());
            }
            runtime.resources().leave_call();
        }
    }
}
