use crate::gc::{CollectionIteration, GcHeap, RootSet};
use crate::value::Value;
use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeInstruction, FunctionRef, LocalSlot, Register,
};
use std::rc::Rc;

use crate::{LoadedModule, ResourceState, RootedInterfaceMethod, Runtime, RuntimeError};

/// An execution scope over the root session's shared frame stack.
/// Dropping it unwinds only the frames entered by this scope.
pub struct ExecutionStack {
    session: crate::ExecutionSession,
    heap: Rc<GcHeap>,
    base: usize,
    id: u64,
}

impl ExecutionStack {
    pub(crate) fn new(
        session: crate::ExecutionSession,
        heap: Rc<GcHeap>,
    ) -> Result<Self, RuntimeError> {
        let base = session
            .state
            .frames
            .try_borrow()
            .map_err(|_| {
                session
                    .resources
                    .quarantine("frame stack is borrowed across execution")
            })?
            .len();
        let id = session.state.next_frame_scope.get();
        session
            .state
            .next_frame_scope
            .set(id.checked_add(1).ok_or_else(|| {
                session
                    .resources
                    .quarantine("frame scope identity exhausted")
            })?);
        session.state.frame_scopes.borrow_mut().push(id);
        Ok(Self {
            session,
            heap,
            base,
            id,
        })
    }

    fn validate_top(&self) -> Result<(), RuntimeError> {
        self.session.resources.ensure_execution_allowed()?;
        if !self
            .session
            .resources
            .active_session()
            .is_some_and(|active| Rc::ptr_eq(&active, &self.session.state))
        {
            return Err(self
                .session
                .resources
                .quarantine("execution used a suspended session"));
        }
        if self.session.state.frame_scopes.borrow().last() != Some(&self.id) {
            return Err(self
                .session
                .resources
                .quarantine("execution used a suspended frame scope"));
        }
        Ok(())
    }

    pub fn frames(&self) -> Result<std::cell::Ref<'_, Vec<ExecutionFrame>>, RuntimeError> {
        self.session.state.frames.try_borrow().map_err(|_| {
            self.session
                .resources
                .quarantine("frame stack is mutably borrowed")
        })
    }

    pub fn current(&self) -> Result<std::cell::Ref<'_, ExecutionFrame>, RuntimeError> {
        self.validate_top()?;
        let frames = self.frames()?;
        if frames.len() <= self.base {
            return Err(self.session.resources.quarantine("missing execution frame"));
        }
        Ok(std::cell::Ref::map(frames, |frames| {
            frames.last().expect("checked frame")
        }))
    }

    pub fn current_mut(&self) -> Result<std::cell::RefMut<'_, ExecutionFrame>, RuntimeError> {
        self.validate_top()?;
        let frames = self.session.state.frames.try_borrow_mut().map_err(|_| {
            self.session
                .resources
                .quarantine("frame stack is borrowed across execution")
        })?;
        if frames.len() <= self.base {
            return Err(self.session.resources.quarantine("missing execution frame"));
        }
        Ok(std::cell::RefMut::map(frames, |frames| {
            frames.last_mut().expect("checked frame")
        }))
    }

    pub fn push(
        &self,
        module: kagari_ir::bytecode::ModuleRef,
        function: FunctionRef,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<(), RuntimeError> {
        self.validate_top()?;
        let loaded = {
            let frames = self.frames()?;
            let caller = (frames.len() > self.base)
                .then(|| frames.last().map(ExecutionFrame::loaded))
                .flatten();
            caller
                .unwrap_or(self.session.root())
                .member(module)
                .ok_or_else(|| self.session.resources.quarantine("invalid frame module"))?
        };
        self.push_resolved(loaded, function, args, return_dst, None)
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
        self.validate_top()?;
        runtime.validate_loaded_module(method.implementation())?;
        runtime.validate_interface_method_arguments(&method, args)?;
        let loaded = method.implementation().clone();
        let function = method.function();
        self.push_resolved(loaded, function, args, return_dst, Some(method))
    }

    pub fn push_closure(
        &self,
        runtime: &Runtime,
        closure: crate::gc::ClosureValueSnapshot,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<(), RuntimeError> {
        self.validate_top()?;
        runtime.validate_loaded_module(&closure.implementation)?;
        let function = closure
            .implementation
            .bytecode
            .functions
            .get(closure.function.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid closure function"))?;
        let all = closure
            .captures
            .into_iter()
            .chain(args.iter().cloned())
            .collect::<Vec<_>>();
        if all.len() != function.metadata.params.len()
            || !all
                .iter()
                .zip(&function.metadata.params)
                .all(|(value, ty)| value.has_representation(*ty))
        {
            return Err(RuntimeError::new(
                crate::RuntimeErrorKind::ScriptTrap,
                "closure call contract mismatch",
            ));
        }
        self.push_resolved(
            closure.implementation,
            closure.function,
            &all,
            return_dst,
            None,
        )
    }

    fn push_resolved(
        &self,
        loaded: LoadedModule,
        function: FunctionRef,
        args: &[Value],
        return_dst: Option<Register>,
        interface_method: Option<RootedInterfaceMethod>,
    ) -> Result<(), RuntimeError> {
        self.validate_top()?;
        if !args
            .iter()
            .all(|value| self.heap.validate_candidate_value(value))
        {
            return Err(RuntimeError::capability_denied(
                "external object in candidate call arguments",
            ));
        }
        let mut frames = self.session.state.frames.try_borrow_mut().map_err(|_| {
            self.session
                .resources
                .quarantine("frame stack is borrowed across execution")
        })?;
        frames
            .try_reserve(1)
            .map_err(|_| self.session.resources.limit("frame capacity"))?;
        self.session.resources.enter_call()?;
        match ExecutionFrame::new(
            self.heap.clone(),
            self.session.resources.clone(),
            loaded,
            function,
            args,
            return_dst,
            interface_method,
        ) {
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
        let mut frames = self.session.state.frames.try_borrow_mut().map_err(|_| {
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
        frames.pop();
        self.session.resources.leave_call();
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool, RuntimeError> {
        Ok(self.frames()?.len() == self.base)
    }
}

impl Drop for ExecutionStack {
    fn drop(&mut self) {
        let mut scopes = self.session.state.frame_scopes.borrow_mut();
        let Some(position) = scopes.iter().position(|id| *id == self.id) else {
            return;
        };
        if position + 1 != scopes.len() {
            self.session
                .resources
                .quarantine("frame scopes dropped out of order");
        }
        scopes.truncate(position);
        let Ok(mut frames) = self.session.state.frames.try_borrow_mut() else {
            self.session
                .resources
                .quarantine("frame stack remained borrowed during cleanup");
            return;
        };
        while frames.len() > self.base {
            frames.pop();
            self.session.resources.leave_call();
        }
    }
}

pub struct ExecutionFrame {
    loaded: LoadedModule,
    function: FunctionRef,
    ip: usize,
    executing: Option<usize>,
    heap: Rc<GcHeap>,
    resources: Rc<ResourceState>,
    slots: RootSet,
    register_count: usize,
    return_dst: Option<Register>,
    interface_method: Option<RootedInterfaceMethod>,
    iterations: Vec<CollectionIteration>,
    key_lookups: Vec<(Value, CollectionIteration)>,
}

impl std::fmt::Debug for ExecutionFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionFrame")
            .field("module", &self.loaded.key())
            .field("function", &self.function)
            .field("ip", &self.ip)
            .finish_non_exhaustive()
    }
}

impl ExecutionFrame {
    pub(crate) fn new(
        heap: Rc<GcHeap>,
        resources: Rc<ResourceState>,
        loaded: LoadedModule,
        function: FunctionRef,
        args: &[Value],
        return_dst: Option<Register>,
        interface_method: Option<RootedInterfaceMethod>,
    ) -> Result<Self, RuntimeError> {
        let metadata = loaded
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(|| resources.quarantine("invalid frame function"))?;
        let expected = usize::from(metadata.parameter_count);
        if args.len() != expected {
            return Err(RuntimeError::module_validation(
                "frame argument count does not match the linked function",
            ));
        }
        let register_count = usize::from(metadata.register_count);
        let mut slots = vec![Value::Unit; register_count + usize::from(metadata.local_count)];
        for (slot, value) in args.iter().enumerate() {
            slots[register_count + slot] = value.clone();
        }

        Ok(Self {
            loaded,
            function,
            ip: 0,
            executing: None,
            heap: heap.clone(),
            resources: resources.clone(),
            slots: heap
                .root_execution_values(slots)
                .ok_or_else(|| RuntimeError::module_validation("invalid heap argument"))?,
            register_count,
            return_dst,
            interface_method,
            iterations: Vec::new(),
            key_lookups: Vec::new(),
        })
    }

    pub fn begin_key_lookup(&mut self, value: &Value) -> Result<(), RuntimeError> {
        self.key_lookups
            .push((value.clone(), self.heap.begin_key_lookup(value)?));
        Ok(())
    }
    pub fn end_key_lookup(&mut self, value: &Value) -> Result<(), RuntimeError> {
        if !self
            .key_lookups
            .last()
            .is_some_and(|(collection, _)| collection == value)
        {
            return Err(RuntimeError::new(
                crate::RuntimeErrorKind::ScriptTrap,
                "key lookup guard mismatch",
            ));
        }
        self.key_lookups.pop();
        Ok(())
    }
    pub fn begin_iteration(&mut self, collection: Register) -> Result<(), RuntimeError> {
        let value = self.read_register(collection)?;
        self.iterations
            .push(self.heap.begin_collection_iteration(&value)?);
        Ok(())
    }

    pub fn end_iteration(&mut self) -> Result<(), RuntimeError> {
        let _guard = self.iterations.pop().ok_or_else(|| {
            RuntimeError::new(
                crate::RuntimeErrorKind::ScriptTrap,
                "iteration guard underflow",
            )
        })?;
        Ok(())
    }

    pub fn next_instruction(&mut self) -> Option<BytecodeInstruction> {
        let instruction = self.function().instructions.get(self.ip).cloned();
        if instruction.is_some() {
            self.executing = Some(self.ip);
            self.ip += 1;
        }
        instruction
    }

    pub fn function(&self) -> &BytecodeFunction {
        &self.loaded.bytecode.functions[self.function.index()]
    }
    pub fn module(&self) -> kagari_ir::bytecode::ModuleRef {
        self.loaded.slot()
    }
    pub fn loaded(&self) -> &LoadedModule {
        &self.loaded
    }

    pub fn interface_method(&self) -> Option<&RootedInterfaceMethod> {
        self.interface_method.as_ref()
    }

    pub(crate) fn set_native_instruction(&mut self, offset: usize) -> Result<(), RuntimeError> {
        if offset >= self.function().instructions.len() {
            return Err(self
                .resources
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

    pub fn jump_to(&mut self, offset: usize) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if offset >= self.function().instructions.len() {
            return Err(self.resources.quarantine("invalid frame jump target"));
        }
        self.ip = offset;
        Ok(())
    }

    pub fn read_register(&self, register: Register) -> Result<Value, RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if register.index() >= self.register_count {
            return Err(self.resources.quarantine("invalid frame register"));
        }
        self.slots
            .get(register.index())
            .ok_or_else(|| self.resources.quarantine("invalid frame register"))
    }

    pub fn write_register(&mut self, register: Register, value: Value) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if register.index() >= self.register_count {
            return Err(self.resources.quarantine("invalid frame register"));
        }
        self.slots
            .set(&self.heap, register.index(), value)
            .ok_or_else(|| self.resources.quarantine("invalid frame register"))
    }

    pub fn read_local(&self, local: LocalSlot) -> Result<Value, RuntimeError> {
        self.slots
            .get(self.register_count + local.index())
            .ok_or_else(|| self.resources.quarantine("invalid frame local"))
    }

    pub fn write_local(&mut self, local: LocalSlot, value: Value) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        self.slots
            .set(&self.heap, self.register_count + local.index(), value)
            .ok_or_else(|| self.resources.quarantine("invalid frame local"))
    }

    pub fn return_dst(&self) -> Option<Register> {
        self.return_dst
    }
}
