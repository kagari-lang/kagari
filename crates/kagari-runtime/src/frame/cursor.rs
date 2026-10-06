//! One checked access to the current frame and its contiguous operand window.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionFrame, ExecutionStack},
    gc::GcHeap,
    session::SessionState,
    value::Value,
};
use kagari_bytecode::instruction::{LocalSlot, Register};
use std::cell::{Ref, RefMut};

/// A transient interpreter view. Release it before GC, observation, native calls,
/// stack growth or synchronous reentry; the stores reject conflicting borrows.
/// Slot access preserves bounds and publication checks without host root leases.
pub struct ExecutionCursor<'a> {
    frame: RefMut<'a, ExecutionFrame>,
    values: RefMut<'a, [Value]>,
    heap: &'a GcHeap,
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
        let values =
            RefMut::filter_map(values, |values| values.get_mut(frame.slots)).map_err(|_| {
                runtime
                    .resources()
                    .quarantine("invalid execution frame window")
            })?;
        Ok(ExecutionCursor {
            frame,
            values,
            heap: runtime.gc(),
            session: self.session.state(),
        })
    }
}

impl ExecutionCursor<'_> {
    fn invalid(&self) -> RuntimeError {
        self.heap
            .resources()
            .quarantine("invalid execution operand slot")
    }

    fn register_index(&self, register: Register) -> Result<usize, RuntimeError> {
        if register.index() < self.frame.register_count {
            Ok(register.index())
        } else {
            Err(self.invalid())
        }
    }

    fn local_index(&self, local: LocalSlot) -> Result<usize, RuntimeError> {
        self.frame
            .register_count
            .checked_add(local.index())
            .ok_or_else(|| self.invalid())
    }

    fn read(&self, index: usize) -> Result<Value, RuntimeError> {
        self.heap.resources().ensure_cursor_allowed(&self.session)?;
        self.values
            .get(index)
            .cloned()
            .ok_or_else(|| self.invalid())
    }

    fn write(&mut self, index: usize, value: Value) -> Result<(), RuntimeError> {
        self.heap.ensure_no_native_borrow()?;
        self.heap.resources().ensure_cursor_allowed(&self.session)?;
        if !self.heap.validate_value(&value) {
            return Err(self.invalid());
        }
        if index >= self.values.len() {
            return Err(self.invalid());
        }
        self.values[index] = value;
        Ok(())
    }

    pub fn read_register(&self, register: Register) -> Result<Value, RuntimeError> {
        self.read(self.register_index(register)?)
    }

    pub fn write_register(&mut self, register: Register, value: Value) -> Result<(), RuntimeError> {
        self.write(self.register_index(register)?, value)
    }

    pub fn read_local(&self, local: LocalSlot) -> Result<Value, RuntimeError> {
        self.read(self.local_index(local)?)
    }

    pub fn write_local(&mut self, local: LocalSlot, value: Value) -> Result<(), RuntimeError> {
        self.write(self.local_index(local)?, value)
    }

    pub fn jump_to(&mut self, target: usize) -> Result<(), RuntimeError> {
        self.heap.resources().ensure_cursor_allowed(&self.session)?;
        if self
            .frame
            .function()
            .is_none_or(|function| target >= function.instructions.len())
        {
            return Err(self
                .heap
                .resources()
                .quarantine("invalid frame jump target"));
        }
        self.frame.ip = target;
        Ok(())
    }
}
