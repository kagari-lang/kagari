//! Checked stack control contains identities and marks, never borrowed storage.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::{roots::SessionId, space},
    execution_metadata::managed::sources::MetadataSource,
    frame::{
        ExecutionFrame, FrameDispatch, FrameEntry, FrameEnvironment, FrameStack,
        arguments::FrameArguments,
    },
    module::{ExecutableModule, managed::source::RevisionSource},
    native::conversion::arguments::ArgumentValues,
    session::SessionState,
    value::{carriers::RootedValue, view::ValueView},
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
        let reserve = {
            let mut scopes = state.frame_scopes.borrow_mut();
            let reserve = space::reserve(&mut scopes, 1, &state.space);
            if reserve.is_ok() {
                scopes.push(id);
            }
            reserve
        };
        drop(state);
        reserve.map_err(|error| runtime.managed_memory_error(error))?;
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

    pub(super) fn validate_top<'r>(
        &self,
        runtime: &'r Runtime,
    ) -> Result<Ref<'r, SessionState>, RuntimeError> {
        let resources = runtime.resources();
        let session = resources.sessions.get(self.session).ok_or_else(|| {
            RuntimeError::module_validation("execution stack belongs to another runtime")
        })?;
        resources.ensure_execution_allowed()?;
        if resources.active_session_id() != Some(self.session) {
            return Err(resources.quarantine(EngineFault::ExecutionUsedASuspendedSession));
        }
        if session.frame_scopes.borrow().last() != Some(&self.id) {
            return Err(resources.quarantine(EngineFault::ExecutionUsedASuspendedFrameScope));
        }
        Ok(session)
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
        runtime: &mut Runtime,
        module: ModuleRef,
        function: FunctionRef,
        args: &[ValueView<'_>],
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
        let origin = runtime.selection_origin(selected.owner())?;
        self.push_arguments(
            runtime,
            selected.owner().clone(),
            selected.target(),
            FrameArguments::plain(args),
            return_dst,
            FrameDispatch {
                origin,
                metadata: None,
                prepared: None,
                entry: FrameEntry::Call,
                invocation: None,
                environment: None,
            },
        )
    }

    /// Enters the current method selected by the rooted interface's checked evidence.
    pub fn push_interface_method(
        &self,
        runtime: &mut Runtime,
        method: &RootedInterfaceMethod,
        args: ArgumentValues<'_>,
        return_dst: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let method = method.admit(runtime)?;
        let view = method.view(runtime)?;
        runtime.validate_executable_module(view.implementation())?;
        let arguments = FrameArguments::pack(args.reborrow())
            .with_interface_receiver(&method._root)
            .map_err(|error| runtime.managed_memory_error(error))?;
        view.validate_arguments(runtime, arguments)?;
        let loaded = view.implementation().clone();
        let target = view.target();
        let source = method
            .computation_root
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("method computation root"))?;
        let metadata = MetadataSource::HostApplication(source);
        let environment = view.environment().map(|facts| FrameEnvironment {
            facts: facts.clone(),
            source: metadata,
        });
        let origin = method.body_origin()?;
        drop(view);
        // The host lease remains alive until the window has published all edges.
        self.push_admitted_arguments(
            runtime,
            loaded,
            target,
            arguments,
            return_dst,
            FrameDispatch {
                origin,
                metadata: Some(metadata),
                prepared: None,
                entry: FrameEntry::Call,
                invocation: Some(method.invocation.clone()),
                environment,
            },
        )
        .map(|_| ())
    }

    /// Enter an already selected exact callable. Retained computations and prepared
    /// host entries must not resolve current code again at this admission boundary.
    pub fn push_callable(
        &self,
        runtime: &mut Runtime,
        loaded: ExecutableModule,
        target: CallableTarget,
        args: ArgumentValues<'_>,
        return_dst: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        // This low-level exact entry borrows an already admitted execution body.
        // A detached descriptor alone cannot reopen a retired publication.
        let origin = RevisionSource::Execution(
            runtime
                .admitted_origin(&loaded)?
                .ok_or_else(|| RuntimeError::module_validation("callable body is not admitted"))?,
        );
        self.push_arguments(
            runtime,
            loaded,
            target,
            FrameArguments::pack(args),
            return_dst,
            FrameDispatch {
                origin,
                metadata: None,
                prepared: None,
                entry: FrameEntry::Call,
                invocation: None,
                environment: None,
            },
        )
    }

    /// Admit an already selected Host submission directly from genuine roots.
    /// Its first callee owns copied fields before this borrowed slice can expire.
    pub(crate) fn push_rooted_callable(
        &self,
        runtime: &mut Runtime,
        loaded: ExecutableModule,
        target: CallableTarget,
        args: &[RootedValue],
    ) -> Result<(), RuntimeError> {
        let origin =
            RevisionSource::Execution(runtime.admitted_origin(&loaded)?.ok_or_else(|| {
                RuntimeError::module_validation("submitted body is not admitted")
            })?);
        self.push_arguments(
            runtime,
            loaded,
            target,
            FrameArguments::retained(args),
            None,
            FrameDispatch {
                origin,
                metadata: None,
                prepared: None,
                entry: FrameEntry::Call,
                invocation: None,
                environment: None,
            },
        )
    }

    pub(super) fn push_arguments(
        &self,
        runtime: &mut Runtime,
        loaded: ExecutableModule,
        target: CallableTarget,
        args: FrameArguments<'_, '_>,
        return_dst: Option<Slot>,
        dispatch: FrameDispatch<'_>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        self.push_admitted_arguments(runtime, loaded, target, args, return_dst, dispatch)
            .map(|_| ())
    }

    pub fn pop(&self, runtime: &mut Runtime) -> Result<(), RuntimeError> {
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
        let retired = frames.pop();
        drop(frames);
        if let Some(mut frame) = retired {
            frame.release_values(&mut runtime.memory, &runtime.resources);
            frame.release_control(&runtime.resources);
        }
        runtime.resources().leave_call();
        Ok(())
    }

    pub fn is_empty(&self, runtime: &Runtime) -> Result<bool, RuntimeError> {
        Ok(self.frames(runtime)?.len() == self.base)
    }
}

impl FrameStack {
    pub(crate) fn retire(&mut self, runtime: &mut Runtime) {
        let Runtime {
            memory, resources, ..
        } = runtime;
        let state = resources
            .sessions
            .get(self.session)
            .expect("live execution scope");
        let mut scopes = state.frame_scopes.borrow_mut();
        let Some(position) = scopes.iter().position(|id| *id == self.id) else {
            return;
        };
        if position + 1 != scopes.len() {
            resources.quarantine(EngineFault::FrameScopesDroppedOutOfOrder);
        }
        scopes.truncate(position);
        if self.parked {
            return;
        }
        let Some(mut frames) = resources.sessions.frames_mut(self.session) else {
            resources.quarantine(EngineFault::FrameStackRemainedBorrowedDuringCleanup);
            return;
        };
        while frames.len() > self.base {
            if let Some(mut frame) = frames.pop() {
                frame.release_values(memory, resources);
                frame.release_control(resources);
            }
            resources.leave_call();
        }
    }
}
