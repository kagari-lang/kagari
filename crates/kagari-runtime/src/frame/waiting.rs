//! A pending Native or Task wait uses the owning execution's protected cells.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::{ExecutionRoots, SessionId, pending::PendingSlots},
    frame::{
        FrameStack, transfer::ReturnValue, types::arguments::TypeArgument,
        values::FrameAllocationError,
    },
    native::{
        binding::NativeResult,
        completion::CompletionRegistry,
        future::{ColdFuture, PendingNative, claims::ClaimedFuture},
    },
    task::waiting::TaskWait,
    value::{carriers::RootedValue, place::ResultPlace},
    value_check,
};
#[cfg(test)]
mod tests;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

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
    slots: PendingSlots,
}

#[derive(Debug)]
enum PendingOperation {
    Native(Box<dyn PendingNative>),
    Task(TaskWait),
    Group { parent: SessionId },
}

/// Branch reservation contains no invocation authority until the atomic pair
/// claim installs it. No reserved branch is scheduled before that commit.
#[derive(Debug)]
pub(crate) enum QueuedFuture {
    Source {
        output: TypeArgument,
    },
    Unclaimed {
        value: RootedValue,
        output: TypeArgument,
    },
    Claimed {
        claim: ClaimedFuture,
        output: TypeArgument,
    },
    Reserved {
        output: TypeArgument,
    },
}

impl PendingWait {
    pub(crate) fn cancel(&mut self) -> NativeResult<()> {
        match &mut self.pending {
            PendingOperation::Native(pending) => pending.cancel(),
            PendingOperation::Task(pending) => {
                pending.cancel();
                Ok(())
            }
            // Parent retirement owns teardown even while the wait is detached.
            PendingOperation::Group { .. } => Ok(()),
        }
    }

    fn poll(&mut self, runtime: &mut Runtime) -> NativeResult<Poll<()>> {
        let output = self.slots.output();
        match &mut self.pending {
            PendingOperation::Native(pending) => {
                match pending.poll(runtime, ResultPlace::new(output))? {
                    Poll::Pending => Ok(Poll::Pending),
                    Poll::Ready(published) => published.admit(output).map(|_| Poll::Ready(())),
                }
            }
            PendingOperation::Group { parent } => match runtime.poll_group(*parent)? {
                Poll::Pending => Ok(Poll::Pending),
                Poll::Ready(root) => {
                    runtime
                        .memory
                        .scope(&runtime.resources, |cx, roots| {
                            ExecutionRoots::with(roots, cx, |slots| {
                                let value = root.get(slots.scope())?;
                                slots.set(output, value)
                            })
                        })
                        .map_err(|error| runtime.managed_memory_error(error))?;
                    Ok(Poll::Ready(()))
                }
            },
            PendingOperation::Task(pending) => {
                pending.poll(runtime, self.slots.awaitable(), output)
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
        registry.check().map_err(|_| {
            self.quarantine_execution_invariant(EngineFault::NativeOperationCleanupFailed)
        })?;
        Ok(registry)
    }
}

impl FrameStack {
    /// The backend advances its PC before entry. The awaitable is published in
    /// pending storage before dead operands are cleared or any callback can run.
    pub fn begin_await(
        &self,
        runtime: &mut Runtime,
        register: Slot,
        destination: Slot,
        future: &Ty,
    ) -> NativeResult<()> {
        self.validate_top(runtime)?;
        runtime.reject_static_initialization("await")?;
        if self.session_state(runtime).pending.borrow().is_some() {
            return Err(RuntimeError::module_validation("execution already waiting"));
        }
        let (frame_slots, location) = {
            let frame = self.current(runtime)?;
            (
                frame.slots(),
                frame.slot_location(runtime.resources(), register, true)?,
            )
        };
        let pending = PendingSlots::new(self.session);
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = frame_slots.read(slots, location)?;
                    pending.clear(slots)?;
                    slots.set(pending.awaitable(), value)
                })
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        self.discard_dead_await_slots(runtime)?;
        if !self.can_park(runtime)? {
            return Err(RuntimeError::module_validation(
                "await requires a suspendable owned execution",
            ));
        }
        runtime.resources.poll_execution()?;
        let (owner, environment) = {
            let frame = self.current(runtime)?;
            (
                frame.loaded.clone(),
                frame.environment().map(|env| env.types.clone()),
            )
        };
        let applied = runtime
            .type_arguments(&owner, environment, slice::from_ref(future))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("await type scope"))?;
        let value = runtime
            .memory
            .inspect_value(pending.awaitable())
            .map_err(|error| runtime.managed_memory_error(error))?;
        if !value_check::matches_protected(runtime, &owner, &applied, &value)? {
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
        let output = applied.parameter(runtime, &owner, 0)?;
        self.start_await(runtime, Some(destination), output, role)
    }

    fn start_await(
        &self,
        runtime: &mut Runtime,
        destination: Option<Slot>,
        output: TypeArgument,
        role: NativeStorageLayout,
    ) -> NativeResult<()> {
        let slots = PendingSlots::new(self.session);
        if role == NativeStorageLayout::Task {
            let wake = self
                .session_state(runtime)
                .owner
                .borrow()
                .as_ref()
                .expect("owned activation")
                .waker();
            let pending = runtime.begin_task_wait(slots.awaitable(), self.session, wake)?;
            *self.session_state(runtime).pending.borrow_mut() = Some(PendingWait {
                destination,
                pending: PendingOperation::Task(pending),
                output,
                slots,
            });
            return Ok(());
        }
        let value = runtime
            .memory
            .inspect_value(slots.awaitable())
            .map_err(|error| runtime.managed_memory_error(error))?;
        let claim = runtime.claim_future(&value)?;
        self.start_claimed(runtime, claim, destination, output)
    }

    fn start_claimed(
        &self,
        runtime: &mut Runtime,
        claim: ClaimedFuture,
        destination: Option<Slot>,
        output: TypeArgument,
    ) -> NativeResult<()> {
        let wake = self
            .session_state(runtime)
            .owner
            .borrow()
            .as_ref()
            .expect("owned activation")
            .waker();
        let slots = PendingSlots::new(self.session);
        let pending = match claim.recipe() {
            ColdFuture::Composition(_) => {
                runtime.request_group(self.session, claim, wake)?;
                PendingOperation::Group {
                    parent: self.session,
                }
            }
            ColdFuture::Script { .. } => {
                self.push_future(runtime, claim, destination, &output)?;
                runtime
                    .memory
                    .scope(&runtime.resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |source| slots.clear(source))
                    })
                    .map_err(|error| runtime.managed_memory_error(error))?;
                return Ok(());
            }
            ColdFuture::Native(_) => {
                let registry = runtime.operation_registry()?.clone();
                let started = claim.start_native(runtime, &registry, &wake);
                registry.check().map_err(|_| {
                    runtime
                        .quarantine_execution_invariant(EngineFault::NativeOperationCleanupFailed)
                })?;
                PendingOperation::Native(started?)
            }
        };
        *self.session_state(runtime).pending.borrow_mut() = Some(PendingWait {
            destination,
            pending,
            output,
            slots,
        });
        // The Native wait/group now owns the computation source. A consumed
        // Future header is no longer an awaitable dependency; Tasks keep theirs.
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |source| slots.release_awaitable(source))
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        Ok(())
    }

    fn discard_dead_await_slots(&self, runtime: &mut Runtime) -> NativeResult<()> {
        let (owner, target, point, frame_slots) = {
            let frame = self.current(runtime)?;
            (
                frame.loaded.clone(),
                frame.target,
                frame.executing,
                frame.slots(),
            )
        };
        let invalid = || RuntimeError::module_validation("await lacks checked suspension facts");
        let CallableTarget::Script(function) = target else {
            return Err(invalid());
        };
        let retained = owner.execution().functions[function.index()]
            .awaits
            .get(&point.ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        let resources = &runtime.resources;
        let result = runtime.memory.scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok(frame_slots.retain_managed(slots, retained, resources))
            })
        });
        result
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| match error {
                FrameAllocationError::Storage(error) => runtime.managed_memory_error(error),
                FrameAllocationError::Execution(error) => {
                    runtime.resources.admit_execution_error(error)
                }
            })
    }

    /// No control, source or payload loan crosses a producer or conversion.
    pub fn poll_await(&self, runtime: &mut Runtime) -> NativeResult<Poll<Option<ReturnValue>>> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::AwaitPoll);
        self.validate_top(runtime)?;
        runtime.resources.poll_execution()?;
        self.start_queued_factory(runtime)?;
        let queued = self
            .session_state(runtime)
            .queued_future
            .borrow_mut()
            .take();
        if let Some(queued) = queued {
            if !self.can_park(runtime)? || !self.frames(runtime)?.is_empty() {
                return Err(RuntimeError::module_validation(
                    "invalid queued Future activation",
                ));
            }
            match queued {
                QueuedFuture::Source { output } => {
                    self.start_await(runtime, None, output, NativeStorageLayout::Future)?;
                }
                QueuedFuture::Claimed { claim, output } => {
                    claim.validate_queued(runtime)?;
                    self.start_claimed(runtime, claim, None, output)?
                }
                QueuedFuture::Unclaimed { value, output } => {
                    let pending = PendingSlots::new(self.session);
                    runtime
                        .memory
                        .scope(&runtime.resources, |cx, roots| {
                            ExecutionRoots::with(roots, cx, |slots| {
                                let value = value.get(slots.scope())?;
                                pending.clear(slots)?;
                                slots.set(pending.awaitable(), value)
                            })
                        })
                        .map_err(|error| runtime.managed_memory_error(error))?;
                    // Pending storage owns the awaitable before its Host lease ends.
                    drop(value);
                    self.start_await(runtime, None, output, NativeStorageLayout::Future)?;
                }
                QueuedFuture::Reserved { .. } => {
                    return Err(RuntimeError::module_validation(
                        "unclaimed branch activation",
                    ));
                }
            }
        }
        let pending = self.session_state(runtime).pending.borrow_mut().take();
        let Some(mut wait) = pending else {
            return Ok(Poll::Ready(None));
        };
        let polled = wait.poll(runtime);
        if polled.is_err() {
            wait.cancel().map_err(|_| {
                runtime.quarantine_execution_invariant(EngineFault::NativeOperationCleanupFailed)
            })?;
        }
        match polled? {
            Poll::Pending => {
                *self.session_state(runtime).pending.borrow_mut() = Some(wait);
                Ok(Poll::Pending)
            }
            Poll::Ready(()) => {
                let owner = if wait.destination.is_some() {
                    self.current(runtime)?.loaded.clone()
                } else {
                    self.session_state(runtime).root.clone()
                };
                let value = runtime
                    .memory
                    .inspect_value(wait.slots.output())
                    .map_err(|error| runtime.managed_memory_error(error))?;
                if !value_check::matches_protected(runtime, &owner, &wait.output, &value)? {
                    return Err(RuntimeError::module_validation(
                        "await output type mismatch",
                    ));
                }
                if let Some(destination) = wait.destination {
                    let (frame, location) = {
                        let frame = self.current(runtime)?;
                        (
                            frame.slots(),
                            frame.slot_location(runtime.resources(), destination, false)?,
                        )
                    };
                    ResultPlace::frame(&frame, Some(&location))
                        .publish(runtime, &value)?
                        .admit_frame(&frame, Some(&location))?;
                    runtime
                        .memory
                        .scope(&runtime.resources, |cx, roots| {
                            ExecutionRoots::with(roots, cx, |slots| wait.slots.clear(slots))
                        })
                        .map_err(|error| runtime.managed_memory_error(error))?;
                    self.complete_source_write(runtime);
                    Ok(Poll::Ready(None))
                } else {
                    let packet = runtime
                        .memory
                        .scope(&runtime.resources, |cx, roots| {
                            ExecutionRoots::with(roots, cx, |slots| {
                                let value = slots.get(wait.slots.output())?;
                                let packet = ReturnValue::protect(slots, self.session, value)?;
                                wait.slots.clear(slots)?;
                                Ok(packet)
                            })
                        })
                        .map_err(|error| runtime.managed_memory_error(error))?;
                    Ok(Poll::Ready(Some(packet)))
                }
            }
        }
    }

    /// Validate fresh data after polling; failure preserves activation for trace
    /// capture and cleanup. A pending wait is required for this transition.
    pub fn park_waiting(activation: &mut Option<Self>, runtime: &mut Runtime) -> NativeResult<()> {
        let stack = activation
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("missing parking activation"))?;
        if !stack.can_park(runtime)? || stack.session_state(runtime).pending.borrow().is_none() {
            return Err(RuntimeError::module_validation(
                "no suspendable pending wait",
            ));
        }
        stack.release_execution_views(runtime)?;
        let mut stack = activation.take().expect("validated pending activation");
        stack.parked = true;
        stack.retire(runtime);
        Ok(())
    }
}
