use crate::{
    RootedInterfaceMethod, Runtime,
    closure::ClosureTarget,
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::MetadataRoot,
    frame::{arguments::FrameArguments, types::TypeEnvironment, values::FrameSlots},
    gc::CollectionIteration,
    module::{LoadedModule, execution::allocation::RegisterAllocation},
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
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{ids::FunctionRef, representation::semantic_representation};
use kagari_types::ty::Ty;
use std::{
    borrow::Cow,
    cell::{Ref, RefMut},
    fmt,
    fmt::{Debug, Formatter},
    ptr,
    sync::Arc,
};

mod arguments;
pub mod cursor;
mod layouts;
mod native;
mod shared;
pub mod types;
pub(crate) mod values;

/// An execution scope over the root session's shared frame stack.
/// Dropping it unwinds only the frames entered by this scope.
pub struct ExecutionStack<'runtime> {
    session: ExecutionSession<'runtime>,
    base: usize,
    id: u64,
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
        Ok(Self { session, base, id })
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
            None,
        )
    }

    /// Copy ordinary script arguments directly from the suspended caller's
    /// window into the callee's reusable arena range. No temporary value vector
    /// or borrowed arena pointer survives stack growth.
    pub fn push_registers(
        &self,
        runtime: &Runtime,
        module: ModuleRef,
        function: FunctionRef,
        registers: &[Register],
        return_dst: Option<Register>,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let (loaded, slots) = {
            let caller = self.current()?;
            if registers
                .iter()
                .any(|register| register.index() >= caller.register_count)
            {
                return Err(runtime
                    .resources()
                    .quarantine("invalid script argument register"));
            }
            (
                caller
                    .loaded
                    .member(module)
                    .ok_or_else(|| runtime.resources().quarantine("invalid script call module"))?,
                caller.slots,
            )
        };
        self.push_arguments(
            runtime,
            loaded,
            CallableTarget::Script(function),
            FrameArguments::frame(slots, registers),
            return_dst,
            FrameDispatch {
                interface_method: None,
                environment: None,
            },
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
        runtime.validate_interface_method_arguments(&method, args)?;
        let loaded = view.implementation().clone();
        let target = view.target();
        drop(view);
        self.push_callable(runtime, loaded, target, args, return_dst, Some(method))
    }

    pub fn push_closure(
        &self,
        runtime: &Runtime,
        value: &Value,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let closure = runtime.resolve_closure(value)?;
        runtime.validate_loaded_module(&closure.implementation)?;
        let script_function = match &closure.target {
            ClosureTarget::Native(native) => {
                if !closure.captures.is_empty()
                    || native.signature.params.len() != args.len()
                    || native
                        .signature
                        .params
                        .iter()
                        .zip(args)
                        .any(|(ty, value)| !ty.matches(runtime, value, &closure.implementation))
                {
                    return Err(RuntimeError::module_validation("native closure arguments"));
                }
                let owner = closure.implementation.clone();
                let target = CallableTarget::Native(native.import);
                let environment = closure.environment.clone();
                drop(closure);
                return self.push_arguments(
                    runtime,
                    owner,
                    target,
                    FrameArguments::plain(args),
                    return_dst,
                    FrameDispatch {
                        interface_method: None,
                        environment,
                    },
                );
            }
            ClosureTarget::Script(function) => *function,
        };
        let function = closure
            .implementation
            .bytecode
            .functions
            .get(script_function.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid closure function"))?;
        let all = FrameArguments::captured(&closure.captures, args)?;
        if all.len() != function.metadata.params.len()
            || !all
                .iter()
                .zip(&function.metadata.params)
                .all(|(value, ty)| value.has_representation(*ty))
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "closure call contract mismatch",
            ));
        }
        if let Some(environment) = &closure.environment {
            for (index, value) in all.iter().enumerate() {
                if let Some(ty) = function.metadata.semantic.params.get(&index)
                    && !(if index < closure.captures.len() {
                        runtime.matches_capture_type(
                            value,
                            ty,
                            &closure.implementation,
                            &environment.types,
                        )
                    } else {
                        runtime.matches_type_in(
                            value,
                            ty,
                            &closure.implementation,
                            Some(&environment.types),
                        )
                    })
                {
                    return Err(RuntimeError::module_validation(
                        "closure semantic argument mismatch",
                    ));
                }
            }
        }
        self.push_arguments(
            runtime,
            closure.implementation.clone(),
            CallableTarget::Script(script_function),
            all,
            return_dst,
            FrameDispatch {
                interface_method: None,
                environment: closure.environment.clone(),
            },
        )
    }

    pub fn push_callable(
        &self,
        runtime: &Runtime,
        loaded: LoadedModule,
        target: CallableTarget,
        args: &[Value],
        return_dst: Option<Register>,
        interface_method: Option<RootedInterfaceMethod>,
    ) -> Result<(), RuntimeError> {
        let environment = interface_method
            .as_ref()
            .and_then(|method| method.environment.clone());
        self.push_arguments(
            runtime,
            loaded,
            target,
            FrameArguments::plain(args),
            return_dst,
            FrameDispatch {
                interface_method,
                environment,
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
        dispatch: FrameDispatch,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        if !args.all(runtime, |value| runtime.gc.validate_candidate_value(value))? {
            return Err(RuntimeError::execution_phase_violation(
                "external object in candidate call arguments",
            ));
        }
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
        frames
            .try_reserve(1)
            .map_err(|_| self.session.resources.limit("frame capacity"))?;
        self.session.resources.enter_call()?;
        let prepared = ExecutionFrame::new(runtime, loaded, target, args, return_dst, dispatch);
        match prepared {
            Ok(frame) => {
                frames.push(frame);
                Ok(())
            }
            Err(error) => {
                self.session.resources.leave_call();
                Err(error)
            }
        }
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
}

#[derive(Debug)]
enum NativeEntryState {
    Script,
    Pending,
    Running,
    Complete,
}

pub struct ExecutionFrame {
    environment: Option<TypeEnvironment>,
    loaded: LoadedModule,
    target: CallableTarget,
    native_entry: NativeEntryState,
    ip: usize,
    executing: Option<usize>,
    slots: FrameSlots,
    register_count: usize,
    registers: Option<Arc<RegisterAllocation>>,
    return_to: ReturnDestination,
    interface_method: Option<RootedInterfaceMethod>,
    iterations: Vec<CollectionIteration>,
    mutations: Vec<(Value, CollectionIteration)>,
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

struct FrameDispatch {
    interface_method: Option<RootedInterfaceMethod>,
    environment: Option<TypeEnvironment>,
}

impl ExecutionFrame {
    pub(crate) fn release_values(&self, resources: &ResourceState) {
        if resources
            .frame_values
            .try_borrow_mut()
            .ok()
            .and_then(|mut values| values.release(self.slots))
            .is_none()
        {
            resources.quarantine("execution slots remained borrowed during frame cleanup");
        }
    }

    fn validate_runtime(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !self.slots.belongs_to(&runtime.gc) {
            return Err(RuntimeError::module_validation(
                "frame belongs to another runtime",
            ));
        }
        runtime.resources().ensure_execution_allowed()
    }

    fn new(
        runtime: &Runtime,
        loaded: LoadedModule,
        target: CallableTarget,
        args: FrameArguments<'_>,
        return_dst: Option<Register>,
        dispatch: FrameDispatch,
    ) -> Result<Self, RuntimeError> {
        let heap = runtime.gc();
        let resources = runtime.resources();
        let FrameDispatch {
            interface_method,
            environment,
        } = dispatch;
        let (register_count, slot_count, argument_offset, native_entry, registers) = match target {
            CallableTarget::Script(function) => {
                let metadata = loaded
                    .bytecode
                    .functions
                    .get(function.index())
                    .ok_or_else(|| resources.quarantine("invalid frame function"))?;
                if args.len() != usize::from(metadata.parameter_count) {
                    return Err(RuntimeError::module_validation(
                        "frame argument count does not match the linked function",
                    ));
                }
                if metadata
                    .metadata
                    .semantic
                    .generic
                    .as_ref()
                    .is_some_and(|body| {
                        environment
                            .as_ref()
                            .is_none_or(|environment| !environment.types.matches(body))
                    })
                {
                    return Err(RuntimeError::module_validation(
                        "shared generic entry requires a call environment",
                    ));
                }
                let register_count = usize::from(metadata.register_count);
                let registers = loaded.execution().functions[function.index()]
                    .registers
                    .clone();
                (
                    register_count,
                    registers.count + usize::from(metadata.local_count),
                    registers.count,
                    NativeEntryState::Script,
                    Some(registers),
                )
            }
            CallableTarget::Native(import) => {
                let import = loaded
                    .bytecode
                    .native_imports
                    .get(import.index())
                    .ok_or_else(|| resources.quarantine("invalid native frame import"))?;
                if import.generic.as_ref().is_some_and(|body| {
                    environment
                        .as_ref()
                        .is_none_or(|environment| !environment.types.matches(body))
                }) {
                    return Err(RuntimeError::module_validation(
                        "shared native frame environment",
                    ));
                }
                let signature = &import.signature;
                if args.len() != signature.params.len() {
                    return Err(RuntimeError::module_validation("native frame arguments"));
                }
                (0, args.len() + 1, 1, NativeEntryState::Pending, None)
            }
        };

        heap.ensure_execution_allowed()?;
        if !args.all(runtime, |value| heap.validate_value(value))? {
            return Err(RuntimeError::module_validation("invalid heap argument"));
        }
        runtime.validate_metadata(MetadataRoot::Program(loaded.clone()).edge())?;
        if let Some(environment) = &environment {
            runtime.validate_metadata(MetadataRoot::Environment(environment.id).edge())?;
        }
        let slots = resources
            .frame_values
            .try_borrow_mut()
            .map_err(|_| resources.quarantine("execution slots borrowed during frame entry"))?
            .allocate(
                slot_count,
                argument_offset,
                &args,
                loaded.clone(),
                environment.clone(),
                registers.clone(),
            )?;
        Ok(Self {
            environment,
            loaded,
            target,
            native_entry,
            ip: 0,
            executing: None,
            slots,
            register_count,
            registers,
            return_to: ReturnDestination::Register(return_dst),
            interface_method,
            iterations: Vec::new(),
            mutations: Vec::new(),
        })
    }

    /// Temporary storage after preparation. Logical register identities in the
    /// function metadata remain stable; named locals occupy separate fixed slots.
    pub fn physical_register_count(&self) -> usize {
        self.registers
            .as_ref()
            .map_or(0, |registers| registers.count)
    }

    pub fn environment(&self) -> Option<TypeEnvironment> {
        self.environment.clone()
    }

    pub fn resolve_type<'a>(
        &self,
        ty: &'a Ty<DefinitionId>,
    ) -> Result<Cow<'a, Ty<DefinitionId>>, RuntimeError> {
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

    pub fn closure_signature<'a>(
        &self,
        register: Register,
        params: &'a [ValueType],
        result: ValueType,
    ) -> Result<(Cow<'a, [ValueType]>, ValueType), RuntimeError> {
        if result != ValueType::Generic && !params.contains(&ValueType::Generic) {
            return Ok((Cow::Borrowed(params), result));
        }
        let function = self
            .function()
            .ok_or_else(|| RuntimeError::module_validation("shared closure call function"))?;
        let semantic = function
            .metadata
            .semantic
            .registers
            .get(&register.index())
            .ok_or_else(|| RuntimeError::module_validation("shared closure call signature"))?;
        let resolved = self.resolve_type(semantic)?;
        let Ty::Function { params, result } = resolved.as_ref() else {
            return Err(RuntimeError::module_validation("shared closure call type"));
        };
        Ok((
            Cow::Owned(params.iter().map(semantic_representation).collect()),
            semantic_representation(result),
        ))
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
            .push((value.clone(), runtime.gc.begin_collection_mutation(value)?));
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

    pub fn begin_iteration(
        &mut self,
        runtime: &Runtime,
        collection: Register,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let value = self.read_register(runtime, collection)?;
        self.iterations
            .push(runtime.gc.begin_collection_iteration(&value)?);
        Ok(())
    }

    pub fn end_iteration(&mut self) -> Result<(), RuntimeError> {
        let _guard = self.iterations.pop().ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "iteration guard underflow")
        })?;
        Ok(())
    }

    pub fn next_instruction(&mut self) -> Option<BytecodeInstruction<DefinitionId>> {
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

    pub fn function(&self) -> Option<&BytecodeFunction<DefinitionId>> {
        match self.target {
            CallableTarget::Script(function) => {
                self.loaded.bytecode.functions.get(function.index())
            }
            CallableTarget::Native(_) => None,
        }
    }

    pub fn native_return(&self, runtime: &Runtime) -> Result<Option<Value>, RuntimeError> {
        self.validate_runtime(runtime)?;
        if !matches!(self.native_entry, NativeEntryState::Complete) {
            return Ok(None);
        }
        self.slots
            .get(&runtime.gc, 0)
            .map(Some)
            .ok_or_else(|| runtime.resources().quarantine("invalid native return slot"))
    }

    pub fn has_pending_native_entry(&self) -> bool {
        matches!(self.native_entry, NativeEntryState::Pending)
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

    pub fn interface_method(&self) -> Option<&RootedInterfaceMethod> {
        self.interface_method.as_ref()
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
