//! A pending native wait is an owned session resource, not a native call frame.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, types::arguments::TypeArgument},
    gc::roots::RootedValue,
    native::{binding::NativeResult, completion::CompletionRegistry, future::PendingNative},
    value::Value,
};
use kagari_bytecode::instruction::Register;
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;
use std::{slice, task::Poll};

#[derive(Debug)]
pub(crate) struct PendingWait {
    destination: Register,
    pending: Box<dyn PendingNative>,
    output: TypeArgument,
    // Keep the Future's exact code/type provenance alive through conversion.
    _future: RootedValue,
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
        let Value::GcHandle(id) = value else {
            return Err(RuntimeError::module_validation("await requires a Future"));
        };
        let root = runtime
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("Future await root"))?;
        let registry = runtime.operation_registry()?;
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
            .root_execution_values(cold.values.clone())
            .ok_or_else(|| RuntimeError::module_validation("Future capture roots"))?;
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

    /// No callback, conversion or result destruction occurs while the session
    /// table is borrowed. This also permits custom conversion to allocate safely.
    pub fn poll_await(&self, runtime: &Runtime) -> NativeResult<Poll<()>> {
        self.validate_runtime(runtime)?;
        runtime.resources().poll_execution()?;
        let pending = self.session.state().pending.borrow_mut().take();
        let Some(mut wait) = pending else {
            return Ok(Poll::Ready(()));
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
                if !wait
                    .output
                    .matches(runtime, &value, self.current()?.loaded())
                {
                    return Err(RuntimeError::module_validation(
                        "await output type mismatch",
                    ));
                }
                self.current_mut()?
                    .write_register(runtime, wait.destination, value)?;
                Ok(Poll::Ready(()))
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
