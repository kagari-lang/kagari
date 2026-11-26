//! Parking retains live values, never transient native borrows.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, trap::TrapReason},
    execution::roots::ExecutionRoots,
    frame::{
        FrameStack, NativeEntryState,
        waiting::{PendingWait, QueuedFuture},
    },
    module::linked_execution::LinkedFunction,
    session::SessionState,
    value_check::{data::AsyncDataWalk, scoped::CheckError},
};
#[cfg(test)]
pub(super) mod tests;

use kagari_bytecode::module::CallableTarget;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{AnyObject, Gc},
};
use std::sync::Arc;

impl FrameStack {
    pub(super) fn refresh_resumed_calls(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        // Refresh every frame, including parents blocked below an awaited child.
        // Bodies, PCs, environments, values and return destinations stay exact.
        let mut links: Vec<(usize, Arc<LinkedFunction>)> = Vec::new();
        for (index, frame) in self.frames(runtime)?.iter().enumerate() {
            // Parking has already detached every execution view. Only bodies
            // with runtime links need preparation or staging on restoration.
            debug_assert!(frame.links.is_none());
            let CallableTarget::Script(function) = frame.target else {
                continue;
            };
            if !frame.loaded.execution().functions[function.index()].needs_runtime_links() {
                continue;
            }
            let prepared = runtime
                .modules
                .linked_function(
                    &frame.loaded,
                    function,
                    frame
                        .environment()
                        .as_ref()
                        .map(|environment| environment.id),
                )
                .ok_or_else(|| RuntimeError::module_validation("missing resumed call table"))?;
            links.push((index, prepared));
        }
        if links.is_empty() {
            return Ok(());
        }
        let mut frames = runtime
            .resources()
            .sessions
            .frames_mut(self.session)
            .ok_or_else(|| {
                runtime.quarantine_execution_invariant(EngineFault::ResumedFrameStackBorrowed)
            })?;
        for (index, links) in links {
            frames[index].links = Some(links);
        }
        drop(frames);
        Ok(())
    }

    pub(super) fn release_execution_views(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        let mut frames = runtime
            .resources()
            .sessions
            .frames_mut(self.session)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::ParkedFrameStackBorrowed)
            })?;
        let previous = frames
            .iter_mut()
            .filter_map(|frame| frame.links.take())
            .collect::<Vec<_>>();
        drop(frames);
        drop(previous);
        Ok(())
    }

    pub(crate) fn can_park(&self, runtime: &mut Runtime) -> Result<bool, RuntimeError> {
        let state = self.validate_top(runtime)?;
        if runtime.static_initialization_active() {
            return Ok(false);
        }
        let (body, has_control_roots) = {
            if state.owner.borrow().is_none()
                || state.scopes.get() != 1
                || state.frame_scopes.borrow().len() != 1
                || !state.host_scopes.borrow().is_empty()
            {
                return Ok(false);
            }
            (
                state.body,
                state.queued_future.borrow().is_some()
                    || state.queued_factory.borrow().is_some()
                    || state
                        .pending
                        .borrow()
                        .as_ref()
                        .and_then(PendingWait::conversion_source)
                        .is_some(),
            )
        };
        drop(state);
        // Shared frame control and physical sources overlap only in this closed
        // provider walk. No executing Runtime service runs; both loans end before
        // error admission or trace capture. Payload loans require exclusive access.
        let Runtime {
            memory,
            resources,
            host_borrows,
            ..
        } = &mut *runtime;
        let frames = resources
            .sessions
            .frames(self.session)
            .ok_or_else(|| resources.quarantine(EngineFault::ParkedFrameStackBorrowed))?;
        // A revision-only prefix and actual inline cells need no payload access.
        // Any additional computation or managed data retains the full walk below.
        if !has_control_roots
            && frames.len() <= 1024
            && !frames
                .iter()
                .any(|frame| matches!(frame.native_entry, NativeEntryState::Running))
        {
            let inline = memory.inline_parking(
                self.session,
                body,
                frames.iter().map(|frame| frame.slots().banks),
            );
            match inline {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(error) => {
                    drop(frames);
                    return Err(runtime.managed_memory_error(error));
                }
            }
        }
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                if slots.native_depth(self.session)? != 0 {
                    return Ok(Ok(false));
                }
                Ok((|| -> Result<bool, CheckError> {
                    let mut walk = AsyncDataWalk::new(host_borrows);
                    for (index, frame) in frames.iter().enumerate() {
                        if index != 0 && index.is_multiple_of(1024) {
                            resources.ensure_execution_allowed()?;
                        }
                        if matches!(frame.native_entry, NativeEntryState::Running) {
                            return Ok(false);
                        }
                        if !walk.frame(slots, self.session, frame.slots().banks, resources)? {
                            return Ok(false);
                        }
                    }
                    if !walk.remaining(slots, self.session, resources)? {
                        return Ok(false);
                    }
                    if has_control_roots {
                        let state = resources
                            .sessions
                            .get(self.session)
                            .expect("validated session");
                        let cx = slots.scope();
                        for root in control_roots(cx, &state)?.into_iter().flatten() {
                            if !walk.object(cx, root, resources)? {
                                return Err(RuntimeError::from(
                                    TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
                                )
                                .into());
                            }
                        }
                    }
                    Ok(true)
                })())
            })
        });
        drop(frames);
        result
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| {
                runtime
                    .resources
                    .admit_execution_error(error.into_runtime(runtime))
            })
    }

    /// Attempt a ready-slice transition, consuming the activation only on success.
    ///
    /// `Ok(false)` preserves the stack so execution can reach a safe boundary.
    /// Errors also preserve it for error admission and trace capture before cleanup;
    /// they do not authorize continuing a faulted execution. An empty slot is invalid.
    /// Success retains frame roots, ends the activation and publishes readiness.
    pub fn try_park_ready(
        activation: &mut Option<Self>,
        runtime: &mut Runtime,
    ) -> Result<bool, RuntimeError> {
        let stack = activation
            .as_mut()
            .ok_or_else(|| RuntimeError::module_validation("missing parking activation"))?;
        if !stack.can_park(runtime)? {
            return Ok(false);
        }
        // No callbacks or execution between proof and this commit. Notification
        // follows the parked flag, so even a wake unwind retains the frame roots.
        stack.release_execution_views(runtime)?;
        stack.parked = true;
        stack
            .session_state(runtime)
            .owner
            .borrow()
            .as_ref()
            .expect("owned activation")
            .mark_ready();
        let mut stack = activation.take().expect("validated activation");
        stack.retire(runtime);
        Ok(true)
    }
}

/// Session control owns these roots outside the parked value banks. Open them
/// in the same closed walk; do not copy them into new Host leases or source cells.
fn control_roots<'gc>(
    cx: &Scope<'gc>,
    state: &SessionState,
) -> Result<[Option<Gc<'gc, AnyObject>>; 3], GcError> {
    let future = match state.queued_future.borrow().as_ref() {
        Some(QueuedFuture::Unclaimed { value, .. }) => value.get(cx)?.object(),
        Some(QueuedFuture::Claimed { claim, .. }) => Some(cx.get(claim.source())?.erase()),
        Some(QueuedFuture::Reserved { .. } | QueuedFuture::Source { .. }) | None => None,
    };
    let callable = state
        .queued_factory
        .borrow()
        .as_ref()
        .and_then(|factory| factory.retained_value())
        .map(|root| root.get(cx))
        .transpose()?
        .and_then(|value| value.object());
    let pending = state
        .pending
        .borrow()
        .as_ref()
        .and_then(PendingWait::conversion_source)
        .map(|source| cx.get(source).map(|cold| cold.erase()))
        .transpose()?;
    Ok([future, callable, pending])
}
