//! All call kinds share retirement and publication after any result adaptation.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        ExecutionStack, ReturnDestination,
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
    },
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

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn finish_return_transport(
        &self,
        runtime: &Runtime,
        packet: ReturnValue,
    ) -> Result<ReturnAction, RuntimeError> {
        self.finish_return(runtime, packet)
    }

    pub(crate) fn finish_return(
        &self,
        runtime: &Runtime,
        mut packet: ReturnValue,
    ) -> Result<ReturnAction, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::CallReturn);
        self.validate_runtime(runtime)?;
        // Poll before borrowing frames so a terminated initializer keeps a complete trace.
        runtime.resources().poll_execution()?;
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
        if frame.initializer.is_some() {
            drop(frames);
            return self.finish_static_return(runtime, packet);
        }
        let destination = frame.return_to;
        let has_destination = !matches!(
            destination,
            ReturnDestination::Slot(None) | ReturnDestination::Prepared(None)
        );
        let mut completion = None;
        if frames.len() == self.base + 1 {
            let value = packet
                .materialize()
                .ok_or_else(|| RuntimeError::module_validation("return representation"))?;
            runtime.gc.publish_value(value)?;
            completion = Some(value);
        } else if has_destination {
            let caller = &frames[frames.len() - 2];
            if let ReturnDestination::Slot(Some(register)) = destination
                && register.index() >= caller.slot_count
            {
                return Err(runtime.resources().quarantine("invalid frame register"));
            }
            // Publish through the common bank boundary while the callee still
            // roots the result. Failure must not retire its window or call depth.
            let mut values = runtime
                .resources()
                .frame_values
                .try_borrow_mut()
                .map_err(|_| {
                    runtime
                        .resources()
                        .quarantine("return operands are borrowed")
                })?;
            let published = if let Some((representation, bits)) = packet.payload() {
                match destination {
                    ReturnDestination::Slot(Some(register)) => values.set_scalar(
                        &runtime.gc,
                        caller.slots,
                        register.index(),
                        representation,
                        bits,
                    ),
                    ReturnDestination::Prepared(Some(location)) => values.set_scalar_location(
                        &runtime.gc,
                        caller.slots,
                        location,
                        representation,
                        bits,
                    ),
                    _ => unreachable!("present return destination"),
                }
            } else {
                let value = packet
                    .materialize()
                    .ok_or_else(|| RuntimeError::module_validation("return representation"))?;
                match destination {
                    ReturnDestination::Slot(Some(register)) => {
                        values.set(&runtime.gc, caller.slots, register.index(), value)
                    }
                    ReturnDestination::Prepared(Some(location)) => {
                        values.set_location(&runtime.gc, caller.slots, location, value)
                    }
                    _ => unreachable!("present return destination"),
                }
            };
            published
                .ok_or_else(|| runtime.resources().quarantine("invalid return destination"))?;
        }
        // No callback or collection separates destination publication and retirement.
        let frame = frames.pop().expect("checked return frame");
        let values = frame.retire_values(self.session.resources);
        drop(frame);
        self.session.resources.leave_call();
        runtime.gc.ensure_execution_allowed()?;
        if frames.len() == self.base {
            drop(values);
            drop(frames);
            let value = completion.expect("prepared completion value");
            return self
                .finish_factory_result(runtime, value)
                .map(|value| match value {
                    Some(value) => ReturnAction::Complete(value),
                    // A factory's completed frame has published its queued Future.
                    None => ReturnAction::Continue(ExecutionAction::Await),
                });
        }
        let caller = frames.last_mut().expect("retained caller");
        if has_destination {
            caller.completed = caller.executing;
        }
        caller.execution_action(runtime).map(ReturnAction::Continue)
    }
}
