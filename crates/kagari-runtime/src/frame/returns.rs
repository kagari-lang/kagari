//! All call kinds share retirement and publication after any result adaptation.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::ExecutionRoots,
    execution_metadata::managed::sources::MetadataSource,
    frame::{
        FrameStack, ReturnDestination,
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
    },
    module::managed::source::RevisionSource,
    native::conversion::context::ConversionContext,
    value::{
        carriers::{RootedValue, ScopedValue},
        place::ResultPlace,
        view::ValueView,
    },
    value_check,
};

use kagari_bytecode::module::CallableTarget;

impl FrameStack {
    /// Finish source access before running a converter or publication service.
    /// The complete activation retains the packet's cell through this callback;
    /// a borrowed result view cannot escape the higher-ranked consumer.
    pub(crate) fn with_result<R>(
        &self,
        runtime: &mut Runtime,
        packet: ReturnValue,
        consume: impl for<'value> FnOnce(&mut Runtime, ValueView<'value>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        self.validate_top(runtime)?;
        let value = runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| packet.view(slots, self.session))
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        consume(runtime, value)
    }

    /// Retain a completed result for ownership beyond this execution activation.
    /// The packet must belong to this stack's current execution; an existing
    /// retained Host packet is authenticated and moved without registering again.
    pub fn retain_result(
        &self,
        runtime: &mut Runtime,
        packet: ReturnValue,
    ) -> Result<RootedValue, RuntimeError> {
        self.validate_top(runtime)?;
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| packet.into_retained(slots, self.session))
            })
            .map_err(|error| runtime.managed_memory_error(error))
    }

    /// Adapt while the callee's arguments and executable dependencies remain rooted.
    /// No exclusive stack or operand-bank borrow crosses an allocating adapter.
    fn adapt_return(
        &self,
        runtime: &mut Runtime,
        packet: ReturnValue,
    ) -> Result<ReturnValue, RuntimeError> {
        let (owner, method, environment, ty, frame_slots) = {
            let frame = self.current(runtime)?;
            let ty = if frame.environment().is_some() {
                Some(
                    match frame.target() {
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
                    .ok_or_else(|| RuntimeError::module_validation("shared return contract"))?
                    .clone(),
                )
            } else {
                None
            };
            (
                frame.loaded().clone(),
                frame.invocation(&runtime.resources),
                frame.admitted_environment().cloned(),
                ty,
                frame.slots(),
            )
        };
        let value = runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| packet.view(slots, self.session))
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        let Some(method) = method else {
            if let (Some(environment), Some(ty)) = (environment, ty) {
                let expected = runtime
                    .type_arguments(&owner, Some(environment.facts().types.clone()), &[ty])?
                    .pop()
                    .ok_or_else(|| RuntimeError::module_validation("shared return contract"))?;
                if !value_check::matches_protected(runtime, &owner, &expected, &value)? {
                    return Err(RuntimeError::module_validation(
                        "shared return type mismatch",
                    ));
                }
            }
            return Ok(packet);
        };
        let computation = method
            .computation()
            .ok_or_else(|| RuntimeError::module_validation("return method is not admitted"))?;
        let metadata = MetadataSource::AdmittedApplication(computation);
        let view = method.view(runtime, &[metadata])?;
        view.validate_result(runtime, &value)?;
        let Some(adapter) = view.result_adapter() else {
            return Ok(packet);
        };
        // Reserve in the frame's own prefix before conversion scratch begins.
        // The adapter publishes here before its dependencies/locals are retired.
        let destination = runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    slots.push_local(self.session, ScopedValue::unit())
                })
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        {
            let mut conversion = ConversionContext::for_selected(
                runtime,
                &owner,
                self.session,
                RevisionSource::Execution(frame_slots.body),
                Some(metadata),
            )?;
            let adapted = conversion.adapt_method_result(adapter, &value, &[metadata])?;
            ResultPlace::new(destination)
                .publish(conversion.runtime_mut(), &adapted)?
                .admit(destination)?;
        }
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let result = slots.get(destination)?;
                    ReturnValue::protect(slots, self.session, result)
                })
            })
            .map_err(|error| runtime.managed_memory_error(error))
    }

    pub fn finish_return(
        &self,
        runtime: &mut Runtime,
        mut packet: ReturnValue,
    ) -> Result<ReturnAction, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::CallReturn);
        self.validate_top(runtime)?;
        // Poll before borrowing frames so a terminated initializer keeps a complete trace.
        runtime.resources().poll_execution()?;
        let adapt = {
            // The control scope was admitted above and polling cannot reenter.
            // Inspect the actual frame without repeating current's complete
            // session/scope admission before this same return transaction.
            let frames = runtime
                .resources
                .sessions
                .frames(self.session)
                .ok_or_else(|| {
                    runtime
                        .resources
                        .quarantine(EngineFault::FrameStackIsMutablyBorrowed)
                })?;
            let frame = frames
                .last()
                .filter(|_| frames.len() > self.base)
                .ok_or_else(|| {
                    runtime
                        .resources
                        .quarantine(EngineFault::MissingExecutionFrame)
                })?;
            frame.environment.is_some() || frame.has_invocation(&runtime.resources)
        };
        if adapt {
            packet = self.adapt_return(runtime, packet)?;
            self.validate_top(runtime)?;
        }
        // Candidate output is rechecked after adaptation and before publication.
        // A mutable/private alias may have changed since argument admission.
        if runtime.resources.candidate_owner().is_some() {
            let value = runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| packet.view(slots, self.session))
                })
                .map_err(|error| runtime.managed_memory_error(error))?;
            value_check::candidate_protected(runtime, &[value])?;
        }
        let resources = &runtime.resources;
        let borrow_frames = || {
            resources
                .sessions
                .frames_mut(self.session)
                .ok_or_else(|| resources.quarantine(EngineFault::ReturnStackIsBorrowed))
        };
        let mut frames = borrow_frames()?;
        let frame = frames
            .last()
            .filter(|_| frames.len() > self.base)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::MissingReturnFrame)
            })?;
        if frame.initializer(resources).is_some() {
            drop(frames);
            return self.finish_static_return(runtime, packet);
        }
        let destination = frame.return_to;
        if frames.len() == self.base + 1 {
            // The activation owns the completed value until its consumer has
            // decoded/published it. Host retention is an explicit consumer step;
            // synchronous Native reentry keeps the surrounding source prefix.
            let transferred = runtime.memory.scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let frame = frames.last_mut().expect("checked return frame");
                    let storage = frame.storage.as_mut().expect("live return storage");
                    let packet = packet.reparent(slots, storage.mark_mut())?;
                    frame
                        .storage
                        .take()
                        .expect("checked return storage")
                        .retire(slots)?;
                    Ok(packet)
                })
            });
            if transferred.is_ok() {
                frames
                    .pop()
                    .expect("checked return frame")
                    .release_control(resources);
                resources.leave_call();
            }
            drop(frames);
            let packet = transferred.map_err(|error| runtime.managed_memory_error(error))?;
            return self
                .finish_factory_result(runtime, packet)
                .map(|value| match value {
                    Some(value) => ReturnAction::Complete(value),
                    None => ReturnAction::Continue(ExecutionAction::Await),
                });
        }
        let caller_index = frames.len() - 2;
        let caller = &frames[caller_index];
        let action = caller.execution_action(runtime)?;
        let location = match destination {
            ReturnDestination::Slot(Some(register)) => Some(
                caller
                    .slot_layout
                    .as_ref()
                    .and_then(|layout| layout.location(register.index()))
                    .ok_or_else(|| resources.quarantine(EngineFault::InvalidFrameRegister))?,
            ),
            ReturnDestination::Prepared(location) => location,
            ReturnDestination::Slot(None) => None,
        };
        let caller_slots = caller.slots();
        let published = runtime.memory.scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let frame = frames.last_mut().expect("checked return frame");
                let storage = frame.storage.as_ref().expect("live return storage");
                packet.publish_to_caller(slots, storage.mark(), caller_slots.banks, location)?;
                // No callback or collector step separates publication from exit.
                frame
                    .storage
                    .take()
                    .expect("checked return storage")
                    .retire(slots)
            })
        });
        if published.is_ok() {
            frames
                .pop()
                .expect("checked return frame")
                .release_control(resources);
            resources.leave_call();
            if location.is_some() {
                let caller = frames.last_mut().expect("retained caller");
                caller.completed = caller.executing;
            }
        }
        drop(frames);
        published.map_err(|error| runtime.managed_memory_error(error))?;
        Ok(ReturnAction::Continue(action))
    }
}
