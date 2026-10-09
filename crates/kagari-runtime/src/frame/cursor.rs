//! One checked access to the current frame and its contiguous operand window.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        ExecutionFrame, ExecutionStack,
        values::{ExecutionValues, WindowRanges},
    },
    session::SessionState,
    value::Value,
};
use kagari_bytecode::instruction::{LocalSlot, Register};
use std::cell::{Ref, RefMut};

pub mod kernel;
mod objects;
mod scalars;

/// A transient interpreter view. Release it before GC, observation, native calls,
/// stack growth or synchronous reentry; the stores reject conflicting borrows.
/// Slot access preserves bounds and publication checks without host root leases.
pub struct ExecutionCursor<'a> {
    frame: RefMut<'a, ExecutionFrame>,
    values: RefMut<'a, ExecutionValues>,
    ranges: WindowRanges,
    runtime: &'a Runtime,
    session: Ref<'a, SessionState>,
}

impl ExecutionStack<'_> {
    pub fn cursor<'a>(&'a self, runtime: &'a Runtime) -> Result<ExecutionCursor<'a>, RuntimeError> {
        self.validate_runtime(runtime)?;
        runtime.gc().ensure_no_native_borrow()?;
        let frame = self.current_mut()?;
        let values = runtime
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| {
                runtime
                    .resources()
                    .quarantine("execution slots borrowed across instruction")
            })?;
        let ranges = values.ranges(frame.slots).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid execution frame window")
        })?;
        Ok(ExecutionCursor {
            frame,
            values,
            ranges,
            runtime,
            session: self.session.state(),
        })
    }
}

impl ExecutionCursor<'_> {
    fn invalid(&self) -> RuntimeError {
        self.runtime
            .gc()
            .resources()
            .quarantine("invalid execution operand slot")
    }

    fn read(&self, logical: usize) -> Result<Value, RuntimeError> {
        self.runtime
            .resources()
            .ensure_cursor_allowed(&self.session)?;
        self.values
            .with_value(self.frame.slots, logical, Value::clone)
            .ok_or_else(|| self.invalid())
    }

    fn write(&mut self, logical: usize, value: Value) -> Result<(), RuntimeError> {
        self.runtime.gc().ensure_no_native_borrow()?;
        self.runtime
            .resources()
            .ensure_cursor_allowed(&self.session)?;
        if !self.runtime.gc().validate_value(&value) {
            return Err(self.invalid());
        }
        self.values
            .set(self.frame.slots, logical, value)
            .ok_or_else(|| self.invalid())
    }

    pub fn read_register(&self, register: Register) -> Result<Value, RuntimeError> {
        if register.index() >= self.frame.register_count {
            return Err(self.invalid());
        }
        self.read(register.index())
    }

    pub fn write_register(&mut self, register: Register, value: Value) -> Result<(), RuntimeError> {
        if register.index() >= self.frame.register_count {
            return Err(self.invalid());
        }
        self.write(register.index(), value)
    }

    pub fn read_local(&self, local: LocalSlot) -> Result<Value, RuntimeError> {
        self.read(self.frame.register_count + local.index())
    }

    pub fn write_local(&mut self, local: LocalSlot, value: Value) -> Result<(), RuntimeError> {
        self.write(self.frame.register_count + local.index(), value)
    }
}
