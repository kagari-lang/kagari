//! The activation alone retires its checked stack suffix before ending a session.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, FrameStack},
    session::ExecutionSession,
};

impl<'runtime> ExecutionStack<'runtime> {
    pub(crate) fn new(session: ExecutionSession<'runtime>) -> Result<Self, RuntimeError> {
        let stack = FrameStack::new(session.runtime, session.scope.id)
            .map_err(|error| error.with_trace(session.runtime.capture_error_trace()))?;
        Ok(Self {
            session,
            stack: Some(stack),
        })
    }

    pub(crate) fn resume(session: ExecutionSession<'runtime>) -> Result<Self, RuntimeError> {
        let mut activation = Self::new(session)?;
        let stack = activation.stack.as_mut().expect("new stack activation");
        // Restoration owns the whole parked suffix. Install cleanup authority
        // before refreshing fallible current-call links, including on errors.
        stack.base = 0;
        stack.refresh_resumed_calls(activation.session.runtime)?;
        Ok(activation)
    }

    /// Borrow checked stack operations together with their owning Runtime.
    /// Values crossing this access must be owned; frame/bank references cannot
    /// escape, and every operation validates its execution identity and scope.
    pub fn with<T>(
        &mut self,
        operation: impl FnOnce(&mut Runtime, &FrameStack) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let stack = self
            .stack
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("parked stack activation"))?;
        debug_assert_eq!(self.session.scope.id, stack.session);
        operation(self.session.runtime, stack)
    }

    pub(crate) fn drive<T>(
        &mut self,
        operation: impl FnOnce(&mut Runtime, &mut Option<FrameStack>) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        operation(self.session.runtime, &mut self.stack)
    }

    /// Cold initialization failure cleanup must end admission before retiring the
    /// owned handle. Keep the Runtime borrow while lending the now-idle services.
    pub(crate) fn retire_then<T>(&mut self, finish: impl FnOnce(&mut Runtime) -> T) -> T {
        if let Some(mut stack) = self.stack.take() {
            stack.retire(self.session.runtime);
        }
        self.session.scope.retire(self.session.runtime);
        finish(self.session.runtime)
    }

    /// Retire a ready activation only after its complete stack is suspendable.
    /// A failed proof leaves the caller's activation available for cleanup.
    pub fn try_park_ready(activation: &mut Option<Self>) -> Result<bool, RuntimeError> {
        let stack = activation
            .as_mut()
            .ok_or_else(|| RuntimeError::module_validation("missing parking activation"))?;
        let parked = stack.drive(|runtime, frames| FrameStack::try_park_ready(frames, runtime))?;
        if parked {
            drop(activation.take());
        }
        Ok(parked)
    }

    /// Park a protected pending wait without consuming its readiness signal.
    pub fn park_waiting(activation: &mut Option<Self>) -> Result<(), RuntimeError> {
        let stack = activation
            .as_mut()
            .ok_or_else(|| RuntimeError::module_validation("missing parking activation"))?;
        stack.drive(|runtime, frames| FrameStack::park_waiting(frames, runtime))?;
        drop(activation.take());
        Ok(())
    }
}

impl Drop for ExecutionStack<'_> {
    fn drop(&mut self) {
        if let Some(mut stack) = self.stack.take() {
            stack.retire(self.session.runtime);
        }
    }
}
