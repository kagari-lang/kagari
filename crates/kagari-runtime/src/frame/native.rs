use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, NativeEntryState, ReturnDestination},
    native::{
        NativeAction, NativeCallback, NativeCallbackTarget, NativeInvocation, NativeProgress,
    },
    value::Value,
};
use kagari_bytecode::{
    instruction::{NativeImportId, Register},
    module::CallableTarget,
};
use std::rc::Rc;

impl ExecutionStack {
    /// Start a native callable frame after the driver charges its entry operation.
    /// No frame borrow crosses the trusted factory or its first state transition.
    pub fn start_native_entry(&self, runtime: &Runtime) -> Result<NativeProgress, RuntimeError> {
        self.validate_native_runtime(runtime)?;
        let (loaded, import, arguments) = {
            let mut frame = self.current_mut()?;
            let CallableTarget::Native(import) = frame.target else {
                return Err(RuntimeError::module_validation(
                    "expected native callable frame",
                ));
            };
            if !matches!(frame.native_entry, NativeEntryState::Pending) {
                return Err(RuntimeError::module_validation(
                    "native callable entry already started",
                ));
            }
            let count = frame.loaded.bytecode.native_imports[import.index()]
                .signature
                .params
                .len();
            let arguments = (1..=count)
                .map(|slot| frame.slots.get(slot))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| RuntimeError::module_validation("native frame argument roots"))?;
            frame.native_entry = NativeEntryState::Running;
            (frame.loaded.clone(), import, arguments)
        };
        let mut invocation = NativeInvocation::start(runtime, loaded, import, &arguments, None)?;
        let action = invocation.take_entry();
        let mut frame = self.current_mut()?;
        frame
            .native
            .try_reserve(1)
            .map_err(|_| self.session.resources.limit("native continuation capacity"))?;
        frame.native.push(invocation);
        drop(frame);
        self.process_native_action(runtime, action)
    }
    fn validate_native_runtime(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.validate_top()?;
        if !Rc::ptr_eq(&runtime.resources, &self.session.resources) {
            return Err(self
                .session
                .resources
                .quarantine("native invocation used another runtime"));
        }
        Ok(())
    }
    /// Entry performs the already charged first logical operation. Further work
    /// is advanced by the driver between safepoints, outside any callback borrow.
    pub fn begin_native(
        &self,
        runtime: &Runtime,
        import: NativeImportId,
        arguments: &[Value],
        destination: Option<Register>,
    ) -> Result<NativeProgress, RuntimeError> {
        self.validate_native_runtime(runtime)?;
        let implementation = self.current()?.loaded().clone();
        let mut invocation =
            NativeInvocation::start(runtime, implementation, import, arguments, destination)?;
        let mut frame = self.current_mut()?;
        if !frame.native.is_empty() {
            return Err(self
                .session
                .resources
                .quarantine("native invocation replaced its continuation"));
        }
        let action = invocation.take_entry();
        frame
            .native
            .try_reserve(1)
            .map_err(|_| self.session.resources.limit("native continuation capacity"))?;
        frame.native.push(invocation);
        drop(frame);
        self.process_native_action(runtime, action)
    }
    pub fn has_native_continuation(&self) -> Result<bool, RuntimeError> {
        Ok(!self.current()?.native.is_empty())
    }
    pub fn advance_native(&self, runtime: &Runtime) -> Result<NativeProgress, RuntimeError> {
        self.validate_native_runtime(runtime)?;
        let mut invocation = self
            .current_mut()?
            .native
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("missing native continuation"))?;
        // No frame borrow spans allocation, callback resolution or trace capture.
        let action = invocation.advance(runtime);
        self.current_mut()?.native.push(invocation);
        self.process_native_action(runtime, action?)
    }
    fn process_native_action(
        &self,
        runtime: &Runtime,
        mut action: NativeAction,
    ) -> Result<NativeProgress, RuntimeError> {
        loop {
            match action {
                NativeAction::Continue => return Ok(NativeProgress::Continue),
                NativeAction::Callback(request) => {
                    return Ok(NativeProgress::Callback(Box::new(request)));
                }
                NativeAction::Complete(value) => {
                    let completed = self.current_mut()?.native.pop().ok_or_else(|| {
                        RuntimeError::module_validation("missing native completion")
                    })?;
                    let destination = completed.destination;
                    if self.current()?.native.is_empty() {
                        if matches!(self.current()?.native_entry, NativeEntryState::Running) {
                            let mut frame = self.current_mut()?;
                            frame.slots.set(&self.heap, 0, value).ok_or_else(|| {
                                RuntimeError::module_validation("native frame result root")
                            })?;
                            frame.native_entry = NativeEntryState::Complete;
                            return Ok(NativeProgress::Finished);
                        }
                        if let Some(destination) = destination {
                            self.current_mut()?.write_register(destination, value)?;
                        }
                        return Ok(NativeProgress::Finished);
                    }
                    action = self.receive_native(runtime, value)?;
                }
            }
        }
    }
    fn receive_native(
        &self,
        runtime: &Runtime,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let mut invocation =
            self.current_mut()?.native.pop().ok_or_else(|| {
                RuntimeError::module_validation("native callback lost its caller")
            })?;
        let action = invocation.receive(runtime, value);
        self.current_mut()?.native.push(invocation);
        action
    }
    pub fn push_native_callback(
        &self,
        runtime: &Runtime,
        request: Box<NativeCallback>,
    ) -> Result<(), RuntimeError> {
        self.validate_native_runtime(runtime)?;
        match request.target {
            NativeCallbackTarget::Closure(closure) => {
                self.push_closure(runtime, closure, &request.arguments, None)?
            }
            NativeCallbackTarget::Interface(method) => {
                self.push_interface_method(runtime, *method, &request.arguments, None)?
            }
            NativeCallbackTarget::Selected {
                implementation,
                target,
            } => {
                runtime.validate_loaded_module(&implementation)?;
                self.push_resolved(implementation, target, &request.arguments, None, None)?;
            }
        }
        self.current_mut()?.return_to = ReturnDestination::Native;
        Ok(())
    }
    /// A reentrant scope returns to its host caller instead of consuming a
    /// suspended outer callback. Nested native steps remain in their owning scope.
    pub fn finish_return(
        &self,
        runtime: &Runtime,
        value: Value,
    ) -> Result<Option<Value>, RuntimeError> {
        self.validate_native_runtime(runtime)?;
        let destination = {
            let frame = self.current()?;
            if let Some(method) = frame.interface_method() {
                runtime.validate_interface_method_result(method, &value)?;
            }
            frame.return_to
        };
        self.pop()?;
        if self.is_empty()? {
            return Ok(Some(value));
        }
        match destination {
            ReturnDestination::Register(Some(destination)) => {
                self.current_mut()?.write_register(destination, value)?
            }
            ReturnDestination::Register(None) => {}
            ReturnDestination::Native => {
                let action = self.receive_native(runtime, value)?;
                match self.process_native_action(runtime, action)? {
                    NativeProgress::Continue | NativeProgress::Finished => {}
                    NativeProgress::Callback(request) => {
                        self.push_native_callback(runtime, request)?
                    }
                }
            }
        }
        Ok(None)
    }
}
