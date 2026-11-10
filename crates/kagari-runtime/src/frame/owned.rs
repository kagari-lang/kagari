//! Parking retains live values, never transient native borrows.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::{ExecutionStack, NativeEntryState},
    module::linked_execution::LinkedFunction,
    session::ExecutionSession,
};
#[cfg(test)]
pub(super) mod tests;

#[cfg(test)]
use crate::gc::retention_probe::{self, Event};

use kagari_bytecode::module::CallableTarget;
use std::sync::Arc;

impl<'runtime> ExecutionStack<'runtime> {
    pub(crate) fn resume(
        session: ExecutionSession<'runtime>,
        runtime: &Runtime,
    ) -> Result<Self, RuntimeError> {
        let mut stack = Self::new(session)?;
        stack.base = 0;
        // Refresh every frame, including parents blocked below an awaited child.
        // Bodies, PCs, environments, values and return destinations stay exact.
        let links = stack
            .frames()?
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
        let mut frames = stack
            .session
            .resources
            .sessions
            .frames_mut(stack.session.id)
            .ok_or_else(|| {
                runtime.quarantine_execution_invariant(EngineFault::ResumedFrameStackBorrowed)
            })?;
        for (frame, links) in frames.iter_mut().zip(links) {
            frame.links = links;
        }
        drop(frames);
        Ok(stack)
    }

    pub(super) fn release_execution_views(&self) -> Result<(), RuntimeError> {
        let mut frames = self
            .session
            .resources
            .sessions
            .frames_mut(self.session.id)
            .ok_or_else(|| {
                self.session
                    .resources
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
        self.validate_runtime(runtime)?;
        runtime.gc().ensure_no_native_borrow()?;
        if runtime.static_initialization_active() {
            return Ok(false);
        }
        let state = self.session.state();
        if state.owner.borrow().is_none()
            || state.scopes.get() != 1
            || state.frame_scopes.borrow().len() != 1
            || !state.host_scopes.borrow().is_empty()
        {
            return Ok(false);
        }
        let frames = self.frames()?;
        let values = self
            .session
            .resources
            .frame_values
            .try_borrow()
            .map_err(|_| {
                self.session
                    .resources
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
                    self.session
                        .resources
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
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("missing parking activation"))?;
        if !stack.can_park(runtime)? {
            return Ok(false);
        }
        // No callbacks or execution between proof and this commit. Notification
        // follows the parked flag, so even a wake unwind retains the frame roots.
        stack.release_execution_views()?;
        let mut stack = activation.take().expect("validated activation");
        stack.parked = true;
        stack
            .session
            .state()
            .owner
            .borrow()
            .as_ref()
            .expect("owned activation")
            .mark_ready();
        drop(stack);
        Ok(true)
    }
}
