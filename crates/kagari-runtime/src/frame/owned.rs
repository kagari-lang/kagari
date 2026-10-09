//! Parking preserves owned iteration resources, never transient native borrows.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, NativeEntryState},
    session::ExecutionSession,
};

impl<'runtime> ExecutionStack<'runtime> {
    pub(crate) fn resume(session: ExecutionSession<'runtime>) -> Result<Self, RuntimeError> {
        let mut stack = Self::new(session)?;
        stack.base = 0;
        Ok(stack)
    }

    pub fn can_park(&self, runtime: &Runtime) -> Result<bool, RuntimeError> {
        self.validate_runtime(runtime)?;
        runtime.gc().ensure_no_native_borrow()?;
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
                    .quarantine("execution windows borrowed during suspension")
            })?;
        for frame in frames.iter() {
            if !frame.mutations.is_empty()
                || matches!(frame.native_entry, NativeEntryState::Running)
            {
                return Ok(false);
            }
            let ranges = values.ranges(frame.slots).ok_or_else(|| {
                self.session
                    .resources
                    .quarantine("invalid parked frame window")
            })?;
            let retained = &values.values[ranges.managed];
            if !retained.iter().all(|value| value.is_storable(runtime.gc())) {
                return Ok(false);
            }
            runtime.gc().validate_async_values(retained)?;
        }
        Ok(true)
    }

    /// Consume the activation while retaining the complete runtime-owned stack.
    pub fn park(mut self, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !self.can_park(runtime)? {
            return Err(RuntimeError::module_validation(
                "execution has non-suspendable resources",
            ));
        }
        self.parked = true;
        self.session
            .state()
            .owner
            .borrow()
            .as_ref()
            .expect("owned activation")
            .mark_ready();
        Ok(())
    }
}
