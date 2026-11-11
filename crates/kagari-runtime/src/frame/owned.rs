//! Parking retains live values, never transient native borrows.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::{FrameStack, NativeEntryState},
    module::linked_execution::LinkedFunction,
};
#[cfg(test)]
pub(super) mod tests;

#[cfg(test)]
use crate::gc::retention_probe::{self, Event};

use kagari_bytecode::module::CallableTarget;
use std::sync::Arc;

impl FrameStack {
    pub(super) fn refresh_resumed_calls(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        // Refresh every frame, including parents blocked below an awaited child.
        // Bodies, PCs, environments, values and return destinations stay exact.
        let links = self
            .frames(runtime)?
            .iter()
            .map(|frame| match frame.target {
                CallableTarget::Script(function)
                    if frame.loaded.execution().functions[function.index()]
                        .needs_runtime_links() =>
                {
                    runtime
                        .modules
                        .linked_function(
                            &frame.loaded,
                            function,
                            frame
                                .environment()
                                .as_ref()
                                .map(|environment| environment.id),
                        )
                        .map(Some)
                        .ok_or_else(|| {
                            RuntimeError::module_validation("missing resumed call table")
                        })
                }
                _ => Ok(None),
            })
            .collect::<Result<Vec<Option<Arc<LinkedFunction>>>, RuntimeError>>()?;
        let mut frames = runtime
            .resources()
            .sessions
            .frames_mut(self.session)
            .ok_or_else(|| {
                runtime.quarantine_execution_invariant(EngineFault::ResumedFrameStackBorrowed)
            })?;
        for (frame, links) in frames.iter_mut().zip(links) {
            frame.links = links;
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

    pub(crate) fn can_park(&self, runtime: &Runtime) -> Result<bool, RuntimeError> {
        #[cfg(test)]
        retention_probe::record(runtime.gc(), Event::Proof);
        self.validate_top(runtime)?;
        runtime.gc().ensure_no_native_borrow()?;
        if runtime.static_initialization_active() {
            return Ok(false);
        }
        let state = self.session_state(runtime);
        if state.owner.borrow().is_none()
            || state.scopes.get() != 1
            || state.frame_scopes.borrow().len() != 1
            || !state.host_scopes.borrow().is_empty()
        {
            return Ok(false);
        }
        let frames = self.frames(runtime)?;
        let values = runtime.resources().frame_values.try_borrow().map_err(|_| {
            runtime
                .resources()
                .quarantine(EngineFault::ExecutionWindowsBorrowedDuringSuspension)
        })?;
        runtime.gc().with_async_values(|walk| {
            for frame in frames.iter() {
                #[cfg(test)]
                retention_probe::record(runtime.gc(), Event::Frame);
                if !frame.mutations.is_empty()
                    || matches!(frame.native_entry, NativeEntryState::Running)
                {
                    return Ok(false);
                }
                let ranges = values.ranges(frame.slots).ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine(EngineFault::InvalidParkedFrameWindow)
                })?;
                let retained = &values.values[ranges.managed];
                if !retained.iter().all(|value| value.is_storable(runtime.gc())) {
                    return Ok(false);
                }
                walk.validate(retained)?;
            }
            Ok(true)
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
        runtime: &Runtime,
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
