use crate::{
    NativeCallback, NativeProgress, Runtime, RuntimeError,
    frame::{ExecutionStack, ReturnDestination},
    native::{NativeAction, NativeCallbackTarget, NativeInvocation},
    value::Value,
};
use kagari_bytecode::{EngineImportId, Register};
use std::rc::Rc;

impl ExecutionStack {
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
        import: EngineImportId,
        arguments: &[Value],
        destination: Option<Register>,
    ) -> Result<NativeProgress, RuntimeError> {
        self.validate_native_runtime(runtime)?;
        let implementation = self.current()?.loaded().clone();
        let mut invocation =
            NativeInvocation::start(runtime, implementation, import, arguments, destination)?;
        let mut frame = self.current_mut()?;
        if frame.native.is_some() {
            return Err(self
                .session
                .resources
                .quarantine("native invocation replaced its continuation"));
        }
        let progress = invocation.take_entry();
        frame.native = Some(invocation);
        Ok(progress)
    }

    pub fn has_native_continuation(&self) -> Result<bool, RuntimeError> {
        Ok(self.current()?.native.is_some())
    }

    pub fn advance_native(&self, runtime: &Runtime) -> Result<NativeProgress, RuntimeError> {
        self.validate_native_runtime(runtime)?;
        let mut invocation = self
            .current_mut()?
            .native
            .take()
            .ok_or_else(|| RuntimeError::module_validation("missing native continuation"))?;
        let destination = invocation.destination;
        // Rooted state remains owned here while allocations capture the caller
        // stack. A frame borrow would hide its origin from Result error traces.
        let action = invocation.advance(runtime);
        let mut frame = self.current_mut()?;
        frame.native = Some(invocation);
        match action? {
            NativeAction::Continue => Ok(NativeProgress::Continue),
            NativeAction::Callback(request) => Ok(NativeProgress::Callback(request)),
            NativeAction::BuiltinFailure(error) => Ok(NativeProgress::BuiltinFailure(error)),
            NativeAction::TypeMismatch(detail) => Ok(NativeProgress::TypeMismatch(detail)),
            NativeAction::Publish(value) => {
                if let Some(destination) = destination {
                    frame.write_register(destination, value)?;
                }
                Ok(NativeProgress::Continue)
            }
            NativeAction::Finish => {
                frame.native = None;
                Ok(NativeProgress::Finished)
            }
            NativeAction::Complete(value) => {
                if let Some(destination) = destination {
                    frame.write_register(destination, value)?;
                }
                frame.native = None;
                Ok(NativeProgress::Finished)
            }
        }
    }

    pub fn push_native_callback(
        &self,
        runtime: &Runtime,
        request: NativeCallback,
    ) -> Result<(), RuntimeError> {
        self.validate_native_runtime(runtime)?;
        match request.target {
            NativeCallbackTarget::Interface(method) => {
                self.push_interface_method(runtime, *method, &request.arguments, None)?;
            }
            NativeCallbackTarget::Closure(closure) => {
                self.push_closure(runtime, closure, &request.arguments, None)?
            }
            NativeCallbackTarget::Function {
                implementation,
                function,
            } => self.push_resolved(implementation, function, &request.arguments, None, None)?,
        }
        self.current_mut()?.return_to = ReturnDestination::Native;
        Ok(())
    }

    /// Complete a script return within this scope. A reentrant scope returns to
    /// its host caller instead of consuming a suspended outer native callback.
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
        let mut frame = self.current_mut()?;
        match destination {
            ReturnDestination::Register(Some(destination)) => {
                frame.write_register(destination, value)?
            }
            ReturnDestination::Register(None) => {}
            ReturnDestination::Native => {
                let invocation = frame.native.as_mut().ok_or_else(|| {
                    RuntimeError::module_validation("native callback lost its caller")
                })?;
                let destination = invocation.destination;
                match invocation.receive(runtime, value)? {
                    NativeAction::Continue => {}
                    NativeAction::Complete(value) => {
                        if let Some(destination) = destination {
                            frame.write_register(destination, value)?;
                        }
                        frame.native = None;
                    }
                    _ => {
                        return Err(RuntimeError::module_validation(
                            "invalid native callback return action",
                        ));
                    }
                }
            }
        }
        Ok(None)
    }
}
