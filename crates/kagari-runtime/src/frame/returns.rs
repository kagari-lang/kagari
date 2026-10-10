//! All call kinds share retirement and publication after any result adaptation.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, ReturnDestination, transfer::ReturnValue},
    value::Value,
};
use kagari_bytecode::module::CallableTarget;

impl ExecutionStack<'_> {
    /// Adapt while the callee's arguments and executable dependencies remain rooted.
    /// No exclusive stack or operand-bank borrow crosses an allocating adapter.
    fn adapt_return(
        &self,
        runtime: &Runtime,
        packet: ReturnValue,
    ) -> Result<ReturnValue, RuntimeError> {
        let mut value = packet
            .materialize()
            .ok_or_else(|| RuntimeError::module_validation("return representation"))?;
        let frame = self.current()?;
        if let Some(method) = frame.invocation() {
            value = runtime.finish_method_result(method, value)?;
        } else if let Some(environment) = frame.environment() {
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
        Ok(ReturnValue::general(value))
    }

    pub fn finish_return(
        &self,
        runtime: &Runtime,
        mut packet: ReturnValue,
    ) -> Result<Option<Value>, RuntimeError> {
        self.validate_runtime(runtime)?;
        let borrow_frames = || {
            self.session
                .resources
                .sessions
                .frames_mut(self.session.id)
                .ok_or_else(|| runtime.resources().quarantine("return stack is borrowed"))
        };
        let mut frames = borrow_frames()?;
        let frame = frames
            .last()
            .filter(|_| frames.len() > self.base)
            .ok_or_else(|| runtime.resources().quarantine("missing return frame"))?;
        if frame.environment.is_some() || frame.invocation.is_some() {
            drop(frames);
            packet = self.adapt_return(runtime, packet)?;
            self.validate_runtime(runtime)?;
            frames = borrow_frames()?;
        }
        let frame = frames
            .last()
            .filter(|_| frames.len() > self.base)
            .ok_or_else(|| runtime.resources().quarantine("missing return frame"))?;
        let destination = frame.return_to;
        let frame = frames.pop().expect("checked return frame");
        frame.release_values(self.session.resources);
        drop(frame);
        self.session.resources.leave_call();
        runtime.gc.ensure_execution_allowed()?;
        if frames.len() == self.base {
            drop(frames);
            let value = packet
                .materialize()
                .ok_or_else(|| RuntimeError::module_validation("return representation"))?;
            return self.finish_factory_result(runtime, value);
        }
        if matches!(
            destination,
            ReturnDestination::Register(None) | ReturnDestination::Prepared(None)
        ) {
            return Ok(None);
        }
        let caller = frames.last().expect("retained caller");
        if let ReturnDestination::Register(Some(register)) = destination
            && register.index() >= caller.register_count
        {
            return Err(runtime.resources().quarantine("invalid frame register"));
        }
        // Retirement and publication form a closed transition: no allocation,
        // collection or callback can observe an unrooted managed return value.
        let mut values = runtime
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| runtime.resources().quarantine("return window borrowed"))?;
        let published = if let Some((representation, bits)) = packet.payload() {
            match destination {
                ReturnDestination::Register(Some(register)) => {
                    values.set_scalar(caller.slots, register.index(), representation, bits)
                }
                ReturnDestination::Prepared(Some(location)) => {
                    values.set_scalar_location(caller.slots, location, representation, bits)
                }
                _ => unreachable!("present return destination"),
            }
        } else {
            let value = packet
                .materialize()
                .ok_or_else(|| RuntimeError::module_validation("return representation"))?;
            if !runtime.gc.validate_value(&value) {
                return Err(runtime.resources().quarantine("invalid return value"));
            }
            match destination {
                ReturnDestination::Register(Some(register)) => {
                    values.set(caller.slots, register.index(), value)
                }
                ReturnDestination::Prepared(Some(location)) => {
                    values.set_location(caller.slots, location, value)
                }
                _ => unreachable!("present return destination"),
            }
        };
        published.ok_or_else(|| runtime.resources().quarantine("invalid return destination"))?;
        Ok(None)
    }
}
