//! An admitted interpreter scope cannot escape the closed execution operation.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        ExecutionFrame, ExecutionStack, cursor::kernel::RegionExit, values::operands::OperandWindow,
    },
    session::SessionState,
    value::Value,
};
use kagari_bytecode::instruction::Register;
use std::ptr;

pub mod kernel;
mod objects;
mod scalars;
#[cfg(test)]
mod tests;

/// All references live inside execute_region. No caller callback or supplied Value
/// can intervene while admission is reused; exits release the frame and bank borrows.
struct ExecutionCursor<'a> {
    frame: &'a mut ExecutionFrame,
    values: OperandWindow<'a>,
    runtime: &'a Runtime,
    session: &'a SessionState,
}

impl ExecutionStack<'_> {
    /// Run a closed region, releasing all transient views before returning a transition.
    /// The driver has already polled and observed the first logical instruction.
    pub fn execute_region(
        &self,
        runtime: &Runtime,
        remaining: &mut Option<usize>,
    ) -> Result<RegionExit, RuntimeError> {
        if !ptr::eq(runtime.resources(), self.session.resources) {
            return Err(RuntimeError::module_validation(
                "execution stack belongs to another runtime",
            ));
        }
        // current_mut admits the active session/frame scope and sticky termination.
        let mut frame = self.current_mut()?;
        runtime.gc().ensure_no_native_borrow()?;
        let mut values = runtime
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| {
                runtime
                    .resources()
                    .quarantine("execution slots borrowed across instruction")
            })?;
        let values = values.borrow_operands(frame.slots).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid execution frame window")
        })?;
        let session = self.session.state();
        ExecutionCursor {
            frame: &mut frame,
            values,
            runtime,
            session: &session,
        }
        .execute_region(remaining)
    }
}

impl ExecutionCursor<'_> {
    fn invalid(&self) -> RuntimeError {
        self.runtime
            .resources()
            .quarantine("invalid execution operand slot")
    }

    fn read_register(&self, register: Register) -> Result<Value, RuntimeError> {
        if register.index() >= self.frame.register_count {
            return Err(self.invalid());
        }
        self.values
            .read(register.index())
            .ok_or_else(|| self.invalid())
    }

    fn write_register(&mut self, register: Register, value: Value) -> Result<(), RuntimeError> {
        if register.index() >= self.frame.register_count
            || !self.runtime.gc().validate_value(&value)
        {
            return Err(self.invalid());
        }
        self.values
            .write(register.index(), value)
            .ok_or_else(|| self.invalid())
    }
}
