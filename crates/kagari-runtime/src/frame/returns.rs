//! Return publication reuses admitted stack access until an adaptation boundary.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, ReturnDestination, transfer::ReturnValue},
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::module::CallableTarget;

enum ScalarReturn {
    Adapt,
    Caller,
    Root,
}

impl ExecutionStack<'_> {
    /// Admission has already checked runtime, active session and scope. Scalar
    /// publication cannot allocate a heap object, adapt a result or call user code.
    fn finish_scalar_return(
        &self,
        runtime: &Runtime,
        representation: ValueType,
        bits: u64,
    ) -> Result<ScalarReturn, RuntimeError> {
        let mut frames = self
            .session
            .resources
            .sessions
            .frames_mut(self.session.id)
            .ok_or_else(|| runtime.resources().quarantine("return stack is borrowed"))?;
        let frame = frames
            .last()
            .filter(|_| frames.len() > self.base)
            .ok_or_else(|| runtime.resources().quarantine("missing return frame"))?;
        if frame.environment.is_some() || frame.interface_method.is_some() {
            return Ok(ScalarReturn::Adapt);
        }
        let destination = frame.return_to;
        let frame = frames.pop().expect("checked return frame");
        frame.release_values(self.session.resources);
        drop(frame);
        self.session.resources.leave_call();
        if frames.len() == self.base {
            return Ok(ScalarReturn::Root);
        }
        if matches!(
            destination,
            ReturnDestination::Register(None) | ReturnDestination::Prepared(None)
        ) {
            return Ok(ScalarReturn::Caller);
        }
        runtime.gc.ensure_execution_allowed()?;
        let caller = frames.last().expect("retained caller");
        let mut values = runtime
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| runtime.resources().quarantine("return window borrowed"))?;
        match destination {
            ReturnDestination::Register(Some(register)) => {
                values.set_scalar(caller.slots, register.index(), representation, bits)
            }
            ReturnDestination::Prepared(Some(location)) => {
                values.set_scalar_location(caller.slots, location, representation, bits)
            }
            _ => unreachable!("present return destination"),
        }
        .ok_or_else(|| RuntimeError::module_validation("scalar return destination"))?;
        Ok(ScalarReturn::Caller)
    }

    pub fn finish_return(
        &self,
        runtime: &Runtime,
        packet: ReturnValue,
    ) -> Result<Option<Value>, RuntimeError> {
        self.validate_runtime(runtime)?;
        if let Some((representation, bits)) = packet.payload() {
            match self.finish_scalar_return(runtime, representation, bits)? {
                ScalarReturn::Adapt => {}
                ScalarReturn::Caller => return Ok(None),
                ScalarReturn::Root => {
                    let value = packet.materialize().ok_or_else(|| {
                        RuntimeError::module_validation("scalar return representation")
                    })?;
                    return self.finish_factory_result(runtime, value);
                }
            }
        }
        let mut value = packet
            .materialize()
            .ok_or_else(|| RuntimeError::module_validation("return representation"))?;
        let destination = {
            let frame = self.current()?;
            if frame.interface_method().is_none()
                && let Some(environment) = frame.environment()
            {
                let ty = match frame.target() {
                    CallableTarget::Script(_) => frame
                        .function()
                        .and_then(|function| function.metadata.semantic.result.as_ref()),
                    CallableTarget::Native(import) => frame
                        .loaded()
                        .bytecode
                        .native_imports
                        .get(import.index())
                        .map(|import| &import.signature.result),
                }
                .ok_or_else(|| RuntimeError::module_validation("shared return contract"))?;
                if !runtime.matches_type_in(&value, ty, frame.loaded(), Some(&environment.types)) {
                    return Err(RuntimeError::module_validation(
                        "shared return type mismatch",
                    ));
                }
            }
            if let Some(method) = frame.interface_method() {
                value = runtime.finish_interface_method_result(method, value)?;
            }
            frame.return_to
        };
        self.pop()?;
        if self.is_empty()? {
            return self.finish_factory_result(runtime, value);
        }
        match destination {
            ReturnDestination::Register(Some(destination)) => {
                self.current_mut()?
                    .write_register(runtime, destination, value)?
            }
            ReturnDestination::Prepared(Some(location)) => {
                let frame = self.current()?;
                frame.validate_runtime(runtime)?;
                frame
                    .slots
                    .set_location(runtime.gc(), location, value)
                    .ok_or_else(|| runtime.resources().quarantine("invalid frame register"))?;
            }
            ReturnDestination::Register(None) | ReturnDestination::Prepared(None) => {}
        }
        Ok(None)
    }
}
