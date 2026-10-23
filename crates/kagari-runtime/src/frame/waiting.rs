//! A pending native or Task wait is an owned session resource, not a call frame.
#[cfg(test)]
mod tests;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, types::arguments::TypeArgument},
    gc::roots::{RootSet, RootedValue},
    native::{
        binding::NativeResult,
        completion::CompletionRegistry,
        future::{ColdFuture, PendingNative},
    },
    session::store::SessionId,
    task::waiting::TaskWait,
    value::Value,
};
use kagari_bytecode::{instruction::Slot, module::CallableTarget};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};
use std::{slice, task::Poll};

#[derive(Debug)]
pub(crate) struct PendingWait {
    destination: Option<Slot>,
    pending: PendingOperation,
    output: TypeArgument,
    // Keep the awaitable's exact code/type provenance and cached result alive.
    awaitable: RootedValue,
}

#[derive(Debug)]
enum PendingOperation {
    Native(Box<dyn PendingNative>),
    Task(TaskWait),
    Group {
        parent: SessionId,
        result: Option<RootedValue>,
    },
}

#[derive(Debug)]
pub(crate) struct QueuedFuture {
    pub value: RootedValue,
    pub output: TypeArgument,
    pub claimed: Option<ClaimedFuture>,
}

#[derive(Debug)]
pub(crate) struct ClaimedFuture {
    pub cold: ColdFuture,
    pub _captures: RootSet,
}

impl PendingWait {
    pub(crate) fn cancel(&mut self) -> NativeResult<()> {
        match &mut self.pending {
            PendingOperation::Native(pending) => pending.cancel(),
            PendingOperation::Task(pending) => {
                pending.cancel();
                Ok(())
            }
            // Parent retirement owns group teardown, including when the wait has
            // been temporarily removed while polling or unwinding an error.
            PendingOperation::Group { .. } => Ok(()),
        }
    }

    fn poll(&mut self, runtime: &Runtime) -> NativeResult<Poll<Value>> {
        match &mut self.pending {
            PendingOperation::Native(pending) => pending.poll(runtime),
            PendingOperation::Group { parent, result } => match runtime.poll_group(*parent)? {
                Poll::Pending => Ok(Poll::Pending),
                Poll::Ready(root) => {
                    let value = root
                        .value(runtime.gc())
                        .ok_or_else(|| RuntimeError::module_validation("group output root"))?;
                    *result = Some(root);
                    Ok(Poll::Ready(value))
                }
            },
            PendingOperation::Task(pending) => {
                let value = self
                    .awaitable
                    .value(runtime.gc())
                    .ok_or_else(|| RuntimeError::module_validation("Task wait root"))?;
                pending.poll(runtime, &value)
            }
        }
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
        destination: Slot,
        future: &Ty,
    ) -> NativeResult<()> {
        self.validate_runtime(runtime)?;
        runtime.reject_static_initialization("await")?;
        self.discard_dead_await_slots(runtime)?;
        if !self.can_park(runtime)? || self.session.state().pending.borrow().is_some() {
            return Err(RuntimeError::module_validation(
                "await requires a suspendable owned execution",
            ));
        }
        runtime.resources().poll_execution()?;
        let (output, role) = {
            let frame = self.current()?;
            let applied = frame
                .type_arguments(runtime, slice::from_ref(future))?
                .pop()
                .ok_or_else(|| RuntimeError::module_validation("await type scope"))?;
            if !applied.matches(runtime, &value, frame.loaded()) {
                return Err(RuntimeError::module_validation("awaitable type mismatch"));
            }
            let Ty::NativeObject(nominal) = applied.ty() else {
                return Err(RuntimeError::module_validation("awaitable storage type"));
            };
            let role = runtime
                .native_entries
                .catalog
                .types
                .get_id(nominal.declaration)
                .map(|ty| ty.kind);
            let Some(TypeDefKind::NativeStorage(
                role @ (NativeStorageLayout::Future | NativeStorageLayout::Task),
            )) = role
            else {
                return Err(RuntimeError::module_validation(
                    "unsupported awaitable storage",
                ));
            };
            (applied.parameter(runtime, frame.loaded(), 0)?, role)
        };
        self.start_await(runtime, value, Some(destination), output, role)
    }

    fn start_await(
        &self,
        runtime: &Runtime,
        value: Value,
        destination: Option<Slot>,
        output: TypeArgument,
        role: NativeStorageLayout,
    ) -> NativeResult<()> {
        let Value::GcHandle(id) = value else {
            return Err(RuntimeError::module_validation(
                "await requires a Future or Task",
            ));
        };
        let root = runtime
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("awaitable root"))?;
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
        if role == NativeStorageLayout::Task {
            let pending = runtime.begin_task_wait(&Value::GcHandle(id), self.session.id, wake)?;
            *self.session.state().pending.borrow_mut() = Some(PendingWait {
                destination,
                pending: PendingOperation::Task(pending),
                output,
                awaitable: root,
            });
            return Ok(());
        }
        let cold = runtime.gc().take_cold_future(id)?;
        // Claim removed the GC edges. Root them before converters or submission.
        let _captures = runtime
            .gc()
            .root_execution_values(cold.values().to_vec())
            .ok_or_else(|| RuntimeError::module_validation("Future capture roots"))?;
        self.start_claimed(runtime, cold, root, destination, output)
    }

    fn start_claimed(
        &self,
        runtime: &Runtime,
        cold: ColdFuture,
        root: RootedValue,
        destination: Option<Slot>,
        output: TypeArgument,
    ) -> NativeResult<()> {
        let wake = self
            .session
            .state()
            .owner
            .borrow()
            .as_ref()
            .expect("owned activation")
            .waker();
        let cold = match cold {
            ColdFuture::Composition(cold) => {
                runtime.request_group(self.session.id, cold, wake)?;
                *self.session.state().pending.borrow_mut() = Some(PendingWait {
                    destination,
                    pending: PendingOperation::Group {
                        parent: self.session.id,
                        result: None,
                    },
                    output,
                    awaitable: root,
                });
                return Ok(());
            }
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
            pending: PendingOperation::Native(pending),
            output,
            awaitable: root,
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
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::AwaitPoll);
        self.validate_runtime(runtime)?;
        runtime.resources().poll_execution()?;
        self.start_queued_factory(runtime)?;
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
            if let Some(claimed) = queued.claimed {
                self.start_claimed(runtime, claimed.cold, queued.value, None, queued.output)?;
            } else {
                self.start_await(
                    runtime,
                    value,
                    None,
                    queued.output,
                    NativeStorageLayout::Future,
                )?;
            }
        }
        let pending = self.session.state().pending.borrow_mut().take();
        let Some(mut wait) = pending else {
            return Ok(Poll::Ready(None));
        };
        let polled = wait.poll(runtime);
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
                        .write_slot(runtime, destination, value)?;
                    Ok(Poll::Ready(None))
                } else {
                    Ok(Poll::Ready(Some(value)))
                }
            }
        }
    }

    /// Prove and consume a pending activation without changing readiness.
    ///
    /// Always validates fresh state after polling; a concurrent completion or
    /// cancellation notification survives the commit. Errors retain the activation
    /// for trace capture and cleanup. Missing/non-suspendable waits are errors.
    pub fn park_waiting(activation: &mut Option<Self>, runtime: &Runtime) -> NativeResult<()> {
        let stack = activation
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("missing parking activation"))?;
        if !stack.can_park(runtime)? || stack.session.state().pending.borrow().is_none() {
            return Err(RuntimeError::module_validation(
                "no suspendable pending wait",
            ));
        }
        let mut stack = activation.take().expect("validated pending activation");
        stack.parked = true;
        drop(stack);
        Ok(())
    }
}
