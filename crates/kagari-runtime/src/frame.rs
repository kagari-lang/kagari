use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::{
        arguments::FrameArguments,
        types::TypeEnvironment,
        values::{ExecutionValues, FrameSlots},
    },
    gc::CollectionMutation,
    module::{
        LoadedModule,
        execution::{
            calls::PreparedCall,
            layout::{FrameLayout, Location},
        },
        linked_execution::LinkedFunction,
    },
    objects::invocation::MethodInvocation,
    resource::ResourceState,
    session::ExecutionSession,
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, LocalSlot, Register},
    module::{BytecodeFunction, CallableTarget},
    program::ModuleRef,
};
use kagari_contract::ids::FunctionRef;
use kagari_types::ty::Ty;
use std::{
    borrow::Cow,
    cell::{Ref, RefMut},
    fmt,
    fmt::{Debug, Formatter},
    ptr,
    sync::Arc,
};

mod admission;
pub(crate) mod arguments;
mod calls;
mod closures;
pub mod cursor;
pub mod driver;
pub(crate) mod factory;
mod fields;
mod future;
mod layouts;
mod native;
mod owned;
mod returns;
mod shared;
pub mod transfer;
pub mod types;
pub(crate) mod values;
pub(crate) mod waiting;

/// An execution scope over the root session's shared frame stack.
/// Dropping it unwinds only the frames entered by this scope.
pub struct ExecutionStack<'runtime> {
    session: ExecutionSession<'runtime>,
    base: usize,
    id: u64,
    parked: bool,
}

impl<'runtime> ExecutionStack<'runtime> {
    pub(crate) fn new(session: ExecutionSession<'runtime>) -> Result<Self, RuntimeError> {
        let base = session
            .resources
            .sessions
            .frames(session.id)
            .ok_or_else(|| {
                session
                    .resources
                    .quarantine("frame stack is borrowed across execution")
            })?
            .len();
        let state = session.state();
        let id = state.next_frame_scope.get();
        state.next_frame_scope.set(id.checked_add(1).ok_or_else(|| {
            session
                .resources
                .quarantine("frame scope identity exhausted")
        })?);
        state.frame_scopes.borrow_mut().push(id);
        drop(state);
        Ok(Self {
            session,
            base,
            id,
            parked: false,
        })
    }

    fn validate_runtime(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.validate_top()?;
        if !ptr::eq(runtime.resources(), self.session.resources) {
            return Err(RuntimeError::module_validation(
                "execution stack belongs to another runtime",
            ));
        }
        Ok(())
    }

    fn validate_top(&self) -> Result<(), RuntimeError> {
        self.session.resources.ensure_execution_allowed()?;
        if !self
            .session
            .resources
            .active_session()
            .is_some_and(|active| active.id == self.session.id)
        {
            return Err(self
                .session
                .resources
                .quarantine("execution used a suspended session"));
        }
        if self.session.state().frame_scopes.borrow().last() != Some(&self.id) {
            return Err(self
                .session
                .resources
                .quarantine("execution used a suspended frame scope"));
        }
        Ok(())
    }

    pub fn frames(&self) -> Result<Ref<'_, Vec<ExecutionFrame>>, RuntimeError> {
        self.session
            .resources
            .sessions
            .frames(self.session.id)
            .ok_or_else(|| {
                self.session
                    .resources
                    .quarantine("frame stack is mutably borrowed")
            })
    }

    pub fn current(&self) -> Result<Ref<'_, ExecutionFrame>, RuntimeError> {
        self.validate_top()?;
        let frames = self.frames()?;
        if frames.len() <= self.base {
            return Err(self.session.resources.quarantine("missing execution frame"));
        }
        Ok(Ref::map(frames, |frames| {
            frames.last().expect("checked frame")
        }))
    }

    pub fn current_mut(&self) -> Result<RefMut<'_, ExecutionFrame>, RuntimeError> {
        self.validate_top()?;
        let frames = self
            .session
            .resources
            .sessions
            .frames_mut(self.session.id)
            .ok_or_else(|| {
                self.session
                    .resources
                    .quarantine("frame stack is borrowed across execution")
            })?;
        if frames.len() <= self.base {
            return Err(self.session.resources.quarantine("missing execution frame"));
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
        return_dst: Option<Register>,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let loaded = {
            let frames = self.frames()?;
            let caller = (frames.len() > self.base)
                .then(|| frames.last().map(ExecutionFrame::loaded))
                .flatten();
            match caller {
                Some(caller) => caller.member(module),
                None => self.session.root().member(module),
            }
            .ok_or_else(|| self.session.resources.quarantine("invalid frame module"))?
        };
        self.push_callable(
            runtime,
            loaded,
            CallableTarget::Script(function),
            args,
            return_dst,
        )
    }

    /// Enters a method selected from a rooted interface value, preserving its
    /// own linked program even when the caller belongs to a newer version.
    pub fn push_interface_method(
        &self,
        runtime: &Runtime,
        method: RootedInterfaceMethod,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let view = method.view(runtime)?;
        runtime.validate_loaded_module(view.implementation())?;
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

    pub fn push_callable(
        &self,
        runtime: &Runtime,
        loaded: LoadedModule,
        target: CallableTarget,
        args: &[Value],
        return_dst: Option<Register>,
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

    fn push_arguments(
        &self,
        runtime: &Runtime,
        loaded: LoadedModule,
        target: CallableTarget,
        args: FrameArguments<'_>,
        return_dst: Option<Register>,
        dispatch: FrameDispatch<'_>,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        self.push_admitted_arguments(runtime, loaded, target, args, return_dst, dispatch)
            .map(|_| ())
    }

    pub fn pop(&self) -> Result<(), RuntimeError> {
        self.validate_top()?;
        let mut frames = self
            .session
            .resources
            .sessions
            .frames_mut(self.session.id)
            .ok_or_else(|| {
                self.session
                    .resources
                    .quarantine("frame stack is borrowed across execution")
            })?;
        if frames.len() <= self.base {
            return Err(self
                .session
                .resources
                .quarantine("frame scope attempted to pop its caller"));
        }
        if let Some(frame) = frames.pop() {
            frame.release_values(self.session.resources);
        }
        self.session.resources.leave_call();
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool, RuntimeError> {
        Ok(self.frames()?.len() == self.base)
    }
}

impl Drop for ExecutionStack<'_> {
    fn drop(&mut self) {
        let state = self.session.state();
        let mut scopes = state.frame_scopes.borrow_mut();
        let Some(position) = scopes.iter().position(|id| *id == self.id) else {
            return;
        };
        if position + 1 != scopes.len() {
            self.session
                .resources
                .quarantine("frame scopes dropped out of order");
        }
        scopes.truncate(position);
        if self.parked {
            return;
        }
        let Some(mut frames) = self.session.resources.sessions.frames_mut(self.session.id) else {
            self.session
                .resources
                .quarantine("frame stack remained borrowed during cleanup");
            return;
        };
        while frames.len() > self.base {
            if let Some(frame) = frames.pop() {
                frame.release_values(self.session.resources);
            }
            self.session.resources.leave_call();
        }
    }
}

#[derive(Clone, Copy)]
enum ReturnDestination {
    Register(Option<Register>),
    Prepared(Option<Location>),
}

#[derive(Debug)]
enum NativeEntryState {
    Script,
    Pending,
    Running,
    Complete,
}

pub struct ExecutionFrame {
    links: Option<Arc<LinkedFunction>>,
    environment: Option<TypeEnvironment>,
    loaded: LoadedModule,
    target: CallableTarget,
    native_entry: NativeEntryState,
    ip: usize,
    executing: Option<usize>,
    slots: FrameSlots,
    register_count: usize,
    registers: Option<Arc<FrameLayout>>,
    return_to: ReturnDestination,
    invocation: Option<MethodInvocation>,
    mutations: Vec<(Value, CollectionMutation)>,
}

impl Debug for ExecutionFrame {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionFrame")
            .field("module", &self.loaded.key())
            .field("target", &self.target)
            .field("ip", &self.ip)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
enum FrameEntry {
    Call,
    Await,
}

struct FrameDispatch<'call> {
    prepared: Option<&'call PreparedCall>,
    entry: FrameEntry,
    invocation: Option<MethodInvocation>,
    environment: Option<TypeEnvironment>,
}

impl ExecutionFrame {
    pub(crate) fn release_values(&self, resources: &ResourceState) {
        drop(self.retire_values(resources));
    }

    /// Retain storage only for the closed retirement/publication transition.
    /// Failure quarantines execution but must not interrupt frame/depth cleanup.
    fn retire_values<'resources>(
        &self,
        resources: &'resources ResourceState,
    ) -> Option<RefMut<'resources, ExecutionValues>> {
        let mut values = resources.frame_values.try_borrow_mut().ok();
        if values
            .as_mut()
            .and_then(|values| values.release(self.slots))
            .is_none()
        {
            resources.quarantine("execution slots remained borrowed during frame cleanup");
        }
        values
    }

    fn validate_runtime(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !self.slots.belongs_to(&runtime.gc) {
            return Err(RuntimeError::module_validation(
                "frame belongs to another runtime",
            ));
        }
        runtime.resources().ensure_execution_allowed()
    }

    /// Temporary storage after preparation. Logical register identities in the
    /// function metadata remain stable; named locals occupy separate fixed slots.
    pub fn physical_register_count(&self) -> usize {
        self.registers
            .as_ref()
            .map_or(0, |registers| registers.count)
    }

    /// Active scalar payload/initialization slots and managed Value slots,
    /// including locals or native arguments. Shared prepared metadata is separate.
    pub fn storage_counts(&self, runtime: &Runtime) -> Result<(usize, usize), RuntimeError> {
        self.validate_runtime(runtime)?;
        let storage = runtime.resources().frame_values.try_borrow().map_err(|_| {
            runtime
                .resources()
                .quarantine("frame storage borrowed during inspection")
        })?;
        let ranges = storage
            .ranges(self.slots)
            .ok_or_else(|| runtime.resources().quarantine("expired frame storage"))?;
        Ok((ranges.scalars.len(), ranges.managed.len()))
    }

    pub fn environment(&self) -> Option<TypeEnvironment> {
        self.environment.clone()
    }

    pub fn resolve_type<'a>(&self, ty: &'a Ty) -> Result<Cow<'a, Ty>, RuntimeError> {
        if ty.is_concrete() {
            return Ok(Cow::Borrowed(ty));
        }
        match &self.environment {
            Some(environment) => environment.types.resolve(ty).map(Cow::Owned),
            None => Err(RuntimeError::module_validation(
                "missing generic call environment",
            )),
        }
    }

    pub fn begin_collection_mutation(
        &mut self,
        runtime: &Runtime,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        self.mutations.try_reserve(1).map_err(|_| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "mutation guard allocation")
        })?;
        self.mutations
            .push((*value, runtime.gc.begin_collection_mutation(value)?));
        Ok(())
    }

    pub fn end_collection_mutation(&mut self, value: &Value) -> Result<(), RuntimeError> {
        if !self
            .mutations
            .last()
            .is_some_and(|(target, _)| target == value)
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "mutation guard mismatch",
            ));
        }
        self.mutations.pop();
        Ok(())
    }

    pub fn next_instruction(&mut self) -> Option<BytecodeInstruction> {
        let instruction = self.function()?.instructions.get(self.ip).cloned();
        if instruction.is_some() {
            self.executing = Some(self.ip);
            self.ip += 1;
        }
        instruction
    }

    pub fn target(&self) -> CallableTarget {
        self.target
    }

    pub fn function(&self) -> Option<&BytecodeFunction> {
        match self.target {
            CallableTarget::Script(function) => {
                self.loaded.bytecode.functions.get(function.index())
            }
            CallableTarget::Native(_) => None,
        }
    }

    pub fn register_type(
        &self,
        runtime: &Runtime,
        register: Register,
    ) -> Result<ValueType, RuntimeError> {
        self.validate_runtime(runtime)?;
        self.function()
            .and_then(|function| function.metadata.registers.get(register.index()))
            .copied()
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine("invalid frame register type")
            })
    }

    pub fn module(&self) -> ModuleRef {
        self.loaded.slot()
    }

    pub fn loaded(&self) -> &LoadedModule {
        &self.loaded
    }

    pub(crate) fn invocation(&self) -> Option<&MethodInvocation> {
        self.invocation.as_ref()
    }

    pub(crate) fn set_native_instruction(
        &mut self,
        runtime: &Runtime,
        offset: usize,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        if self
            .function()
            .is_none_or(|function| offset >= function.instructions.len())
        {
            return Err(runtime
                .resources()
                .quarantine("invalid native instruction offset"));
        }
        self.executing = Some(offset);
        Ok(())
    }

    pub fn instruction_offset(&self) -> usize {
        self.executing.unwrap_or(self.ip)
    }

    /// Advance the observable program point only when this frame resumes.
    pub fn prepare_instruction(&mut self) {
        self.executing = None;
    }

    pub fn jump_to(&mut self, runtime: &Runtime, offset: usize) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        if self
            .function()
            .is_none_or(|function| offset >= function.instructions.len())
        {
            return Err(runtime.resources().quarantine("invalid frame jump target"));
        }
        self.ip = offset;
        Ok(())
    }

    pub fn read_register(
        &self,
        runtime: &Runtime,
        register: Register,
    ) -> Result<Value, RuntimeError> {
        self.validate_runtime(runtime)?;
        if register.index() >= self.register_count {
            return Err(runtime.resources().quarantine("invalid frame register"));
        }
        self.slots
            .get(&runtime.gc, register.index())
            .ok_or_else(|| runtime.resources().quarantine("invalid frame register"))
    }

    pub fn write_register(
        &mut self,
        runtime: &Runtime,
        register: Register,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        if register.index() >= self.register_count {
            return Err(runtime.resources().quarantine("invalid frame register"));
        }
        self.slots
            .set(&runtime.gc, register.index(), value)
            .ok_or_else(|| runtime.resources().quarantine("invalid frame register"))
    }

    pub fn read_local(&self, runtime: &Runtime, local: LocalSlot) -> Result<Value, RuntimeError> {
        self.validate_runtime(runtime)?;
        self.slots
            .get(&runtime.gc, self.register_count + local.index())
            .ok_or_else(|| runtime.resources().quarantine("invalid frame local"))
    }

    pub fn write_local(
        &mut self,
        runtime: &Runtime,
        local: LocalSlot,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        self.slots
            .set(&runtime.gc, self.register_count + local.index(), value)
            .ok_or_else(|| runtime.resources().quarantine("invalid frame local"))
    }
}

impl Runtime {
    /// Native backends publish their logical program point before a cancellation/GC safepoint.
    pub(crate) fn record_native_instruction(&self, offset: usize) -> Result<(), RuntimeError> {
        let session = self.resources().active_session().ok_or_else(|| {
            self.resources()
                .quarantine("native program point without an execution session")
        })?;
        let mut frames = self
            .resources()
            .sessions
            .frames_mut(session.id)
            .ok_or_else(|| {
                self.resources()
                    .quarantine("native program point while frames are borrowed")
            })?;
        let frame = frames.last_mut().ok_or_else(|| {
            self.resources()
                .quarantine("native program point without an execution frame")
        })?;
        frame.set_native_instruction(self, offset)
    }
}

#[cfg(test)]
mod execution_frames;
