//! A pending native wait is an owned session resource, not a native call frame.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, types::arguments::TypeArgument},
    gc::roots::RootedValue,
    native::{
        binding::NativeResult,
        completion::CompletionRegistry,
        future::{ColdFuture, PendingNative},
    },
    value::Value,
};
use kagari_bytecode::{instruction::Register, module::CallableTarget};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;
use std::{slice, task::Poll};

#[derive(Debug)]
pub(crate) struct PendingWait {
    destination: Option<Register>,
    pending: Box<dyn PendingNative>,
    output: TypeArgument,
    // Keep the Future's exact code/type provenance alive through conversion.
    _future: RootedValue,
}

#[derive(Debug)]
pub(crate) struct QueuedFuture {
    pub value: RootedValue,
    pub output: TypeArgument,
}

impl PendingWait {
    pub(crate) fn cancel(&mut self) -> NativeResult<()> {
        self.pending.cancel()
    }
}

impl Runtime {
    pub(crate) fn operation_registry(&self) -> NativeResult<&CompletionRegistry> {
        let registry = self
            .operations
            .get_or_init(|| CompletionRegistry::new(self.async_limits.max_pending_operations))
            .as_ref()
            .map_err(Clone::clone)?;
        registry
            .check()
            .map_err(|_| self.quarantine_execution_invariant("native operation cleanup failed"))?;
        Ok(registry)
    }
}

impl ExecutionStack<'_> {
    /// Start a single await. Repeated polling uses `poll_await`, never this entry.
    /// The backend advances its PC before entering and publishes no destination
    /// value until the resumed completion is successfully converted.
    pub fn begin_await(
        &self,
        runtime: &Runtime,
        value: Value,
        destination: Register,
        future: &Ty<DefinitionId>,
    ) -> NativeResult<()> {
        self.validate_runtime(runtime)?;
        self.discard_dead_await_slots(runtime)?;
        if !self.can_park(runtime)? || self.session.state().pending.borrow().is_some() {
            return Err(RuntimeError::module_validation(
                "await requires a suspendable owned execution",
            ));
        }
        runtime.resources().poll_execution()?;
        let output = {
            let frame = self.current()?;
            let applied = frame
                .type_arguments(runtime, slice::from_ref(future))?
                .pop()
                .ok_or_else(|| RuntimeError::module_validation("await type scope"))?;
            if !applied.matches(runtime, &value, frame.loaded()) {
                return Err(RuntimeError::module_validation(
                    "await Future type mismatch",
                ));
            }
            applied.parameter(runtime, frame.loaded(), 0)?
        };
        self.start_await(runtime, value, Some(destination), output)
    }

    fn start_await(
        &self,
        runtime: &Runtime,
        value: Value,
        destination: Option<Register>,
        output: TypeArgument,
    ) -> NativeResult<()> {
        let Value::GcHandle(id) = value else {
            return Err(RuntimeError::module_validation("await requires a Future"));
        };
        let root = runtime
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("Future await root"))?;
        runtime
            .gc()
            .validate_async_values(slice::from_ref(&Value::GcHandle(id)))?;
        let wake = self
            .session
            .state()
            .owner
            .borrow()
            .as_ref()
            .expect("owned activation")
            .waker();
        let cold = runtime.gc().take_cold_future(id)?;
        // Claim removed the GC edges. Root them before converters or submission.
        let _captures = runtime
            .gc()
            .root_execution_values(cold.values().to_vec())
            .ok_or_else(|| RuntimeError::module_validation("Future capture roots"))?;
        let cold = match cold {
            ColdFuture::Script(closure) => {
                return self.push_future(runtime, closure, destination, &output);
            }
            ColdFuture::Native(cold) => cold,
        };
        let registry = runtime.operation_registry()?;
        let started = cold.start(runtime, registry, &wake);
        registry.check().map_err(|_| {
            runtime.quarantine_execution_invariant("native operation cleanup failed")
        })?;
        let pending = started?;
        *self.session.state().pending.borrow_mut() = Some(PendingWait {
            destination,
            pending,
            output,
            _future: root,
        });
        Ok(())
    }

    fn discard_dead_await_slots(&self, runtime: &Runtime) -> NativeResult<()> {
        let frame = self.current()?;
        let invalid = || RuntimeError::module_validation("await lacks checked suspension facts");
        let CallableTarget::Script(function) = frame.target else {
            return Err(invalid());
        };
        let point = frame.executing.ok_or_else(invalid)?;
        let retained = frame.loaded.execution().functions[function.index()]
            .awaits
            .get(&point)
            .ok_or_else(invalid)?;
        runtime
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| {
                runtime
                    .resources()
                    .quarantine("execution windows borrowed during await")
            })?
            .retain_managed(frame.slots, retained)
            .ok_or_else(invalid)
    }

    /// No callback, conversion or result destruction occurs while the session
    /// table is borrowed. This also permits custom conversion to allocate safely.
    pub fn poll_await(&self, runtime: &Runtime) -> NativeResult<Poll<Option<Value>>> {
        self.validate_runtime(runtime)?;
        runtime.resources().poll_execution()?;
        let queued = self.session.state().queued_future.borrow_mut().take();
        if let Some(queued) = queued {
            if !self.can_park(runtime)? || !self.frames()?.is_empty() {
                return Err(RuntimeError::module_validation(
                    "invalid queued Future activation",
                ));
            }
            let value = queued
                .value
                .value(runtime.gc())
                .ok_or_else(|| RuntimeError::module_validation("queued Future root"))?;
            self.start_await(runtime, value, None, queued.output)?;
        }
        let pending = self.session.state().pending.borrow_mut().take();
        let Some(mut wait) = pending else {
            return Ok(Poll::Ready(None));
        };
        let polled = wait.pending.poll(runtime);
        if polled.is_err() {
            wait.cancel().map_err(|_| {
                runtime.quarantine_execution_invariant("native operation cleanup failed")
            })?;
        }
        match polled? {
            Poll::Pending => {
                *self.session.state().pending.borrow_mut() = Some(wait);
                Ok(Poll::Pending)
            }
            Poll::Ready(value) => {
                let owner = if wait.destination.is_some() {
                    self.current()?.loaded().clone()
                } else {
                    self.session.root()
                };
                if !wait.output.matches(runtime, &value, &owner) {
                    return Err(RuntimeError::module_validation(
                        "await output type mismatch",
                    ));
                }
                if let Some(destination) = wait.destination {
                    self.current_mut()?
                        .write_register(runtime, destination, value)?;
                    Ok(Poll::Ready(None))
                } else {
                    Ok(Poll::Ready(Some(value)))
                }
            }
        }
    }

    /// Park after a pending poll without overwriting readiness published by a
    /// concurrent completion or cancellation. Slices use `park` and stay ready.
    pub fn park_waiting(mut self, runtime: &Runtime) -> NativeResult<()> {
        if !self.can_park(runtime)? || self.session.state().pending.borrow().is_none() {
            return Err(RuntimeError::module_validation(
                "no suspendable pending wait",
            ));
        }
        self.parked = true;
        Ok(())
    }
}
